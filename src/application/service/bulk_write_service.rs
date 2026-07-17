//! The hand-authored bulk-ops write path (user-owned; survives regen).
//!
//! Mass back-office operations as AUDITED, IDEMPOTENT batch jobs that drive each module's write path (never
//! raw SQL, never bypassing invariants). A job holds N items; `run_job` applies each **once** through a
//! `BulkTargetPort` — idempotent per (job, item_key) so a re-run never doubles an item — and records each
//! outcome. A failed item is isolated (recorded, the batch continues). Posts NO GL.

use backbone_orm::company_scope;
use sqlx::{PgPool, Row};
use uuid::Uuid;

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
}

impl BulkWriteService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
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
        sqlx::query(
            r#"INSERT INTO bulkops.bulk_jobs
                 (id, company_id, operation_type, target_module, status, total_items, succeeded_count,
                  failed_count, submitted_by)
               VALUES ($1,$2,$3,$4,'pending'::bulk_job_status,0,0,0,$5)"#,
        )
        .bind(job_id).bind(j.company_id).bind(&j.operation_type).bind(&j.target_module).bind(j.submitted_by)
        .execute(&mut *tx).await?;

        let mut inserted = 0i32;
        for it in &j.items {
            if it.item_key.trim().is_empty() {
                return Err(BulkError::Invalid("an item needs an item_key".into()));
            }
            let n = sqlx::query(
                r#"INSERT INTO bulkops.bulk_job_items (id, job_id, item_key, status, payload)
                   VALUES ($1,$2,$3,'pending'::bulk_item_status,$4)
                   ON CONFLICT (job_id, item_key) DO NOTHING"#,
            )
            .bind(Uuid::new_v4()).bind(job_id).bind(&it.item_key).bind(it.payload.to_string())
            .execute(&mut *tx).await?;
            inserted += n.rows_affected() as i32;
        }
        sqlx::query("UPDATE bulkops.bulk_jobs SET total_items=$2 WHERE id=$1")
            .bind(job_id).bind(inserted).execute(&mut *tx).await?;
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
        let job = company_scope::fetch_optional_row_scoped(
            &self.pool,
            sqlx::query(
                "SELECT company_id, operation_type FROM bulkops.bulk_jobs WHERE id=$1 AND (metadata->>'deleted_at') IS NULL")
                .bind(job_id),
        ).await?
            .ok_or(BulkError::NotFound("job"))?;
        let company_id: Uuid = job.get("company_id");
        let operation_type: String = job.get("operation_type");

        company_scope::execute_scoped(
            &self.pool,
            sqlx::query("UPDATE bulkops.bulk_jobs SET status='running'::bulk_job_status WHERE id=$1")
                .bind(job_id),
        ).await?;

        let items = company_scope::fetch_all_rows_scoped(
            &self.pool,
            sqlx::query(
                r#"SELECT id, item_key, payload FROM bulkops.bulk_job_items
                   WHERE job_id=$1 AND status='pending'::bulk_item_status"#,
            )
            .bind(job_id),
        ).await?;

        let (mut succeeded, mut failed) = (0i32, 0i32);
        for it in &items {
            let item_id: Uuid = it.get("id");
            let item_key: String = it.get("item_key");
            let payload: serde_json::Value = serde_json::from_str::<serde_json::Value>(&it.get::<String, _>("payload"))
                .unwrap_or(serde_json::Value::Null);
            // RESERVE the item BEFORE the external apply — a CAS `pending → applying`. Only the reserver
            // calls `port.apply()`; a concurrent runner that already loaded this item as pending loses the
            // CAS (0 rows) and skips it, so the target is applied at most once even under two concurrent
            // runs. (The CAS was previously AFTER apply, which protected the COUNT but not the APPLY —
            // maturity council 2026-07-10.)
            let reserved = company_scope::execute_scoped(
                &self.pool,
                sqlx::query(
                    r#"UPDATE bulkops.bulk_job_items SET status='applying'::bulk_item_status
                       WHERE id=$1 AND status='pending'::bulk_item_status"#,
                )
                .bind(item_id),
            ).await?;
            if reserved.rows_affected() != 1 {
                continue; // another runner owns this item
            }

            let op = BulkOp { company_id, operation_type: operation_type.clone(), item_key, payload };
            match port.apply(&op).await {
                Ok(ack) => {
                    company_scope::execute_scoped(
                        &self.pool,
                        sqlx::query(
                            r#"UPDATE bulkops.bulk_job_items
                               SET status='applied'::bulk_item_status, applied_ref_type=$2, applied_ref_id=$3
                               WHERE id=$1 AND status='applying'::bulk_item_status"#,
                        )
                        .bind(item_id).bind(&ack.applied_ref_type).bind(ack.applied_ref_id),
                    ).await?;
                    succeeded += 1;
                }
                Err(rej) => {
                    company_scope::execute_scoped(
                        &self.pool,
                        sqlx::query(
                            r#"UPDATE bulkops.bulk_job_items
                               SET status='failed'::bulk_item_status, error_detail=$2
                               WHERE id=$1 AND status='applying'::bulk_item_status"#,
                        )
                        .bind(item_id).bind(&rej.message),
                    ).await?;
                    failed += 1;
                }
            }
        }

        // Roll the job counts up from the item ledger (authoritative — covers prior runs too).
        let counts = company_scope::fetch_one_row_scoped(
            &self.pool,
            sqlx::query(
                r#"SELECT
                     count(*) FILTER (WHERE status='applied'::bulk_item_status) AS applied,
                     count(*) FILTER (WHERE status='failed'::bulk_item_status)  AS failed
                   FROM bulkops.bulk_job_items WHERE job_id=$1"#,
            )
            .bind(job_id),
        ).await?;
        let applied_total: i64 = counts.get("applied");
        let failed_total: i64 = counts.get("failed");
        let job_status = if failed_total > 0 { "failed" } else { "completed" };
        company_scope::execute_scoped(
            &self.pool,
            sqlx::query(
                r#"UPDATE bulkops.bulk_jobs
                   SET status=$2::bulk_job_status, succeeded_count=$3, failed_count=$4 WHERE id=$1"#,
            )
            .bind(job_id).bind(job_status).bind(applied_total as i32).bind(failed_total as i32),
        ).await?;

        let total_items: i32 = company_scope::fetch_one_scalar_scoped(
            &self.pool,
            sqlx::query_scalar("SELECT total_items FROM bulkops.bulk_jobs WHERE id=$1")
                .bind(job_id),
        ).await?;
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
        let rows = company_scope::fetch_all_rows_scoped(
            &self.pool,
            sqlx::query(
                r#"SELECT item_key, error_detail, payload FROM bulkops.bulk_job_items
                   WHERE job_id=$1 AND status='failed'::bulk_item_status
                   ORDER BY item_key"#,
            )
            .bind(job_id),
        ).await?;
        Ok(rows.iter().map(|r| FailedItem {
            item_key: r.get("item_key"),
            error_detail: r.get("error_detail"),
            payload: serde_json::from_str(&r.get::<String, _>("payload")).unwrap_or(serde_json::Value::Null),
        }).collect())
    }

    /// Reset a job's FAILED items back to `pending` so `run_job` retries just them (after the operator fixes
    /// the cause). Without this a batch with failures is a dead end — a re-run skips `failed` items
    /// (completeness council 2026-07-10). Returns the number requeued.
    pub async fn retry_failed(&self, job_id: Uuid) -> Result<u64, BulkError> {
        // RLS scope (ADR-0008), ID-only pattern — see `run_job`.
        let moved = company_scope::execute_scoped(
            &self.pool,
            sqlx::query(
                r#"UPDATE bulkops.bulk_job_items
                   SET status='pending'::bulk_item_status, error_detail=NULL
                   WHERE job_id=$1 AND status='failed'::bulk_item_status"#,
            )
            .bind(job_id),
        ).await?;
        Ok(moved.rows_affected())
    }
}
