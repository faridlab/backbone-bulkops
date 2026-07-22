//! The hand-authored bulk-ops write path (user-owned; survives regen).
//!
//! Mass back-office operations as AUDITED, IDEMPOTENT batch jobs that drive each module's write path (never
//! raw SQL, never bypassing invariants). A job holds N items; `run_job` applies each **once** through a
//! `BulkTargetPort` — idempotent per (job, item_key) so a re-run never doubles an item — and records each
//! outcome. A failed item is isolated (recorded, the batch continues). Posts NO GL.

use backbone_orm::company_scope;
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
}

pub struct NewItem {
    pub item_key: String,
    pub payload: serde_json::Value,
}

pub struct NewJob {
    pub company_id: Uuid,
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
        // RLS scope (ADR-0008): the job carries its company on the DTO, so bind it explicitly onto this
        // transaction's connection — every INSERT below is then fenced by `app.company_id`.
        company_scope::bind_company_on(&mut tx, j.company_id).await?;
        self.jobs.insert_job(&mut tx, &NewJobRow {
            id: job_id,
            company_id: j.company_id,
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
                company_id: j.company_id,
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
    pub async fn run_job(
        &self,
        job_id: Uuid,
        port: &dyn BulkTargetPort,
        events: &dyn BulkEventSink,
    ) -> Result<RunSummary, BulkError> {
        // RLS scope (ADR-0008), ID-only pattern: a run is identified by the job id alone — there is no
        // company argument to scope from up front. This read therefore rides the REQUEST-dedicated
        // connection (which carries the caller's `app.company_id`), so another company's job is simply
        // not found. When driven by a JOB/EVENT rather than HTTP, the CALLER must wrap this call in
        // `with_company_scope(Some(company_id))` or the reads fail closed.
        let job = self
            .jobs
            .fetch_for_run(&self.pool, job_id)
            .await?
            .ok_or(BulkError::NotFound("job"))?;
        let company_id = job.company_id;
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
        let job_status = if failed_total > 0 { "failed" } else { "completed" };
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
        // RLS scope (ADR-0008), ID-only pattern — see `run_job`: fenced by the request-dedicated
        // connection, so another company's job reports no failures.
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
        // RLS scope (ADR-0008), ID-only pattern — see `run_job`.
        let moved = self.items.requeue_failed(&self.pool, job_id).await?;
        Ok(moved)
    }
}
