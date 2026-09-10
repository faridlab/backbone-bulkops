//! The hand-authored bulk-ops write path (user-owned; survives regen).
//!
//! Mass back-office operations as AUDITED, IDEMPOTENT batch jobs that drive each module's write path (never
//! raw SQL, never bypassing invariants). A job holds N items; `run_job` applies each **once** through a
//! `BulkTargetPort` — idempotent per (job, item_key) so a re-run never doubles an item — and records each
//! outcome. A failed item is isolated (recorded, the batch continues). Posts NO GL.

use backbone_orm::org_scope;
use sqlx::PgPool;
use uuid::Uuid;

use crate::infrastructure::persistence::{
    BulkJobItemRepository, BulkJobRepository, NewItemRow, NewJobRow,
};

use super::bulk_events::*;
use super::bulk_ports::*;

#[derive(Debug, thiserror::Error)]
pub enum BulkError {
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    #[error("not found: {0}")]
    NotFound(&'static str),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error(
        "no company-anchored org scope on the caller: the target write path is keyed by the \
         legacy company id, so a run needs a session scope that carries it"
    )]
    NoCompanyScope,
}

pub struct NewItem {
    pub item_key: String,
    pub payload: serde_json::Value,
}

pub struct NewJob {
    pub operation_type: String,
    pub target_module: String,
    pub submitted_by: Option<Uuid>,
    pub items: Vec<NewItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunSummary {
    pub job_id: Uuid,
    pub succeeded: i32,
    pub failed: i32,
    pub skipped: i32,
}

/// What a reconcile pass did with a job's stranded `applying` items.
#[derive(Debug, Clone, PartialEq)]
pub struct ReconcileSummary {
    pub job_id: Uuid,
    /// The re-check confirmed the target already held the effect — marked applied, NOT re-applied.
    pub confirmed: i32,
    /// The target held nothing for the key — re-applied through the port (the item's own outcome,
    /// applied or failed, is recorded on it like any run).
    pub reapplied: i32,
    /// The target could not determine the item — cancelled to the terminal state; an operator decides
    /// whether to resubmit.
    pub cancelled: i32,
    /// Items still short of a terminal state after this pass (not stale yet, or awaiting a run).
    pub unfinished: i32,
}

/// The job's terminal status from the ledger tally: a failure dominates; otherwise a reconciled
/// cancellation marks the job `cancelled` (an operator must see the unverifiable item); only a clean
/// ledger completes.
fn terminal_status(failed: i64, cancelled: i64) -> &'static str {
    if failed > 0 {
        "failed"
    } else if cancelled > 0 {
        "cancelled"
    } else {
        "completed"
    }
}

/// A failed item, surfaced so the operator can act on it without touching the private ledger.
#[derive(Debug, Clone, PartialEq)]
pub struct FailedItem {
    pub item_key: String,
    pub error_detail: Option<String>,
    pub payload: serde_json::Value,
}

pub struct BulkWriteService {
    pool: PgPool,
    jobs: BulkJobRepository,
    items: BulkJobItemRepository,
}

impl BulkWriteService {
    pub fn new(pool: PgPool) -> Self {
        let jobs = BulkJobRepository::new(pool.clone());
        let items = BulkJobItemRepository::new(pool.clone());
        Self { pool, jobs, items }
    }

    /// Create an audited batch of `pending` items. Items dedup on (job, item_key), so a duplicate key
    /// within the batch is collapsed. Requires ≥1 item.
    pub async fn create_job(&self, j: NewJob) -> Result<Uuid, BulkError> {
        if j.operation_type.trim().is_empty() {
            return Err(BulkError::Invalid("job needs an operation_type".into()));
        }
        if j.items.is_empty() {
            return Err(BulkError::Invalid("a job needs at least one item".into()));
        }
        let job_id = Uuid::new_v4();
        let mut tx = self.pool.begin().await?;
        // Tenancy posture (ADR-0029): the module owns no scoping column — the composing
        // service's tenancy decorator does. Relay the AMBIENT request scope onto this
        // transaction when the caller bound one, so the decorator's org-unit fill (and any
        // policy it installed) sees this transaction's inserts. An undecorated deployment
        // has no ambient scope and skips this entirely.
        if let Some(scope) = org_scope::current_org_scope() {
            org_scope::bind_org_scope_on(&mut tx, &scope).await?;
        }
        self.jobs.insert_job(&mut tx, &NewJobRow {
            id: job_id,
            operation_type: &j.operation_type,
            target_module: &j.target_module,
            submitted_by: j.submitted_by,
        }).await?;

        let mut inserted = 0i32;
        for it in &j.items {
            if it.item_key.trim().is_empty() {
                return Err(BulkError::Invalid("an item needs an item_key".into()));
            }
            let n = self.items.insert_item(&mut tx, &NewItemRow {
                id: Uuid::new_v4(),
                job_id,
                item_key: &it.item_key,
                payload: &it.payload.to_string(),
            }).await?;
            inserted += n as i32;
        }
        self.jobs.set_total_items(&mut tx, job_id, inserted).await?;
        tx.commit().await?;
        Ok(job_id)
    }

    /// Run a batch: apply each PENDING item through the target write path, recording each outcome. A failed
    /// item is isolated (recorded `failed`, the batch continues). Idempotent — a re-run processes only
    /// still-`pending` items, so an already-applied item is never re-applied. Emits `BulkJobCompleted`.
    ///
    /// Tenancy posture (ADR-0029): the module owns no scoping column, so the fetch, the item
    /// reservations, and the outcome writes all ride the CALLER'S ambient org scope — a composing
    /// service establishes it (e.g. `with_org_request_scope`) and the database fence it installed
    /// owns tenant isolation; another tenant's job is simply not found (`NotFound`). The target
    /// write path is the SIBLING module's domain, though: during the tenancy transition its rows
    /// still key on the legacy company id, so the key handed to the port is sourced from the
    /// ambient scope's company twin and the run FAILS CLOSED (`NoCompanyScope`) when the caller
    /// carries no company-anchored scope — never a guessed tenant.
    pub async fn run_job(
        &self,
        job_id: Uuid,
        port: &dyn BulkTargetPort,
        events: &dyn BulkEventSink,
    ) -> Result<RunSummary, BulkError> {
        // The legacy company key the target write path keys on (see the posture note above).
        let company_id = org_scope::current_org_scope()
            .and_then(|s| s.legacy_company_id())
            .ok_or(BulkError::NoCompanyScope)?;

        let job = self
            .jobs
            .fetch_for_run(&self.pool, job_id)
            .await?
            .ok_or(BulkError::NotFound("job"))?;
        let operation_type = job.operation_type;

        self.jobs.mark_running(&self.pool, job_id).await?;

        let items = self.items.fetch_pending(&self.pool, job_id).await?;

        let (mut succeeded, mut failed) = (0i32, 0i32);
        for it in &items {
            let item_id = it.id;
            let item_key = it.item_key.clone();
            let payload: serde_json::Value =
                serde_json::from_str::<serde_json::Value>(&it.payload).unwrap_or(serde_json::Value::Null);
            // RESERVE the item BEFORE the external apply — a CAS `pending → applying`. Only the reserver
            // calls `port.apply()`; a concurrent runner that already loaded this item as pending loses the
            // CAS (0 rows) and skips it, so the target is applied at most once even under two concurrent
            // runs. (The CAS was previously AFTER apply, which protected the COUNT but not the APPLY —
            // maturity council 2026-07-10.)
            let reserved = self.items.reserve(&self.pool, item_id).await?;
            if reserved != 1 {
                continue; // another runner owns this item
            }

            let op = BulkOp { company_id, operation_type: operation_type.clone(), item_key, payload };
            match port.apply(&op).await {
                Ok(ack) => {
                    self.items
                        .mark_applied(&self.pool, item_id, &ack.applied_ref_type, ack.applied_ref_id)
                        .await?;
                    succeeded += 1;
                }
                Err(rej) => {
                    self.items.mark_failed(&self.pool, item_id, &rej.message).await?;
                    failed += 1;
                }
            }
        }

        // Roll the job counts up from the item ledger (authoritative — covers prior runs too).
        let counts = self.items.count_outcomes(&self.pool, job_id).await?;
        let applied_total = counts.applied;
        let failed_total = counts.failed;
        let job_status = terminal_status(failed_total, counts.cancelled);
        self.jobs
            .set_outcome(&self.pool, job_id, job_status, applied_total as i32, failed_total as i32)
            .await?;

        let total_items = self.jobs.fetch_total_items(&self.pool, job_id).await?;
        events.publish(&BulkEvent::BulkJobCompleted(BulkJobCompleted {
            job_id, company_id, operation_type,
            total_items, succeeded_count: applied_total as i32, failed_count: failed_total as i32,
        }));

        Ok(RunSummary { job_id, succeeded, failed, skipped: items.len() as i32 - succeeded - failed })
    }

    /// The failure report for a job — the failed items with their key, error, and payload — so the operator
    /// can see what/why WITHOUT querying the private item ledger (completeness council 2026-07-10).
    pub async fn failures(&self, job_id: Uuid) -> Result<Vec<FailedItem>, BulkError> {
        // Tenancy posture (ADR-0029) — see `run_job`: the read rides the caller's ambient org
        // scope, so another tenant's job reports no failures.
        let rows = self.items.fetch_failed(&self.pool, job_id).await?;
        Ok(rows.into_iter().map(|r| FailedItem {
            item_key: r.item_key,
            error_detail: r.error_detail,
            payload: serde_json::from_str(&r.payload).unwrap_or(serde_json::Value::Null),
        }).collect())
    }

    /// Reset a job's FAILED items back to `pending` so `run_job` retries just them (after the operator fixes
    /// the cause). Without this a batch with failures is a dead end — a re-run skips `failed` items
    /// (completeness council 2026-07-10). Returns the number requeued.
    pub async fn retry_failed(&self, job_id: Uuid) -> Result<u64, BulkError> {
        // Tenancy posture (ADR-0029) — see `run_job`.
        let moved = self.items.requeue_failed(&self.pool, job_id).await?;
        Ok(moved)
    }

    /// Recover a crashed run: the job's `applying` items older than `older_than` are stranded — the
    /// runner died between the target's commit and the engine's outcome mark. Each is re-checked
    /// against the target by `item_key`, NEVER blind-re-applied:
    ///
    /// - target already holds the effect → marked `applied` (with the target's original ref);
    /// - target holds nothing for the key → re-applied through the port (safe: `apply` is idempotent
    ///   on `(company_id, item_key)` by the port contract, so even a wrong answer cannot double the
    ///   effect) and the item records that outcome;
    /// - target cannot determine it → the item exits to the terminal `cancelled` state — the engine
    ///   neither duplicates an effect nor silently drops one; an operator decides whether to resubmit.
    ///
    /// If the pass empties the job's non-terminal set, the job rolls up from the ledger and publishes
    /// `BulkJobCompleted` (same contract as `run_job` — downstream mapping persistence gates on it).
    ///
    /// Tenancy posture (ADR-0029): the pass scopes exactly as `run_job` does — the module's own
    /// reads and writes ride the caller's ambient org scope (a mismatched tenant is indistinguishable
    /// from a missing job, `NotFound`), and the legacy company key handed to the port comes from that
    /// scope's company twin, failing closed (`NoCompanyScope`) when there is none.
    pub async fn reconcile_applying(
        &self,
        job_id: Uuid,
        port: &dyn BulkTargetPort,
        events: &dyn BulkEventSink,
        older_than: chrono::Duration,
    ) -> Result<ReconcileSummary, BulkError> {
        if older_than < chrono::Duration::zero() {
            return Err(BulkError::Invalid("older_than must not be negative".into()));
        }
        // The legacy company key the target write path keys on (see the posture note above).
        let company_id = org_scope::current_org_scope()
            .and_then(|s| s.legacy_company_id())
            .ok_or(BulkError::NoCompanyScope)?;

        let job = self
            .jobs
            .fetch_for_run(&self.pool, job_id)
            .await?
            .ok_or(BulkError::NotFound("job"))?;
        let operation_type = job.operation_type;

        let stale = self
            .items
            .fetch_applying_stale(&self.pool, job_id, older_than.num_seconds())
            .await?;

        let (mut confirmed, mut reapplied, mut cancelled) = (0i32, 0i32, 0i32);
        for it in &stale {
            match port.check_applied(company_id, &it.item_key).await {
                // The target already holds the effect — record its ref; never re-apply.
                Ok(Some(ack)) => {
                    self.items
                        .mark_applied(&self.pool, it.id, &ack.applied_ref_type, ack.applied_ref_id)
                        .await?;
                    confirmed += 1;
                }
                // The target holds nothing for the key — a re-apply cannot double the effect.
                Ok(None) => {
                    let payload: serde_json::Value = serde_json::from_str::<serde_json::Value>(&it.payload)
                        .unwrap_or(serde_json::Value::Null);
                    let op = BulkOp {
                        company_id,
                        operation_type: operation_type.clone(),
                        item_key: it.item_key.clone(),
                        payload,
                    };
                    match port.apply(&op).await {
                        Ok(ack) => {
                            self.items
                                .mark_applied(&self.pool, it.id, &ack.applied_ref_type, ack.applied_ref_id)
                                .await?;
                        }
                        Err(rej) => {
                            self.items.mark_failed(&self.pool, it.id, &rej.message).await?;
                        }
                    }
                    reapplied += 1;
                }
                // The target cannot answer — cancel; never guess an effect into or out of existence.
                Err(rej) => {
                    self.items
                        .mark_cancelled(&self.pool, it.id, &format!("unverifiable after a crashed run: {}", rej.message))
                        .await?;
                    cancelled += 1;
                }
            }
        }

        // A pass that emptied the non-terminal set finishes the job: roll up from the ledger and
        // publish completion under the same contract as `run_job`.
        let unfinished = self.items.count_unfinished(&self.pool, job_id).await?;
        if unfinished == 0 {
            let counts = self.items.count_outcomes(&self.pool, job_id).await?;
            self.jobs
                .set_outcome(
                    &self.pool,
                    job_id,
                    terminal_status(counts.failed, counts.cancelled),
                    counts.applied as i32,
                    counts.failed as i32,
                )
                .await?;
            let total_items = self.jobs.fetch_total_items(&self.pool, job_id).await?;
            events.publish(&BulkEvent::BulkJobCompleted(BulkJobCompleted {
                job_id, company_id, operation_type,
                total_items, succeeded_count: counts.applied as i32, failed_count: counts.failed as i32,
            }));
        }

        Ok(ReconcileSummary {
            job_id,
            confirmed,
            reapplied,
            cancelled,
            unfinished: unfinished as i32,
        })
    }
}
