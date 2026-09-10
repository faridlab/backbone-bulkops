//! Golden cases — the batch oracle: run applies each item through the target write path; a failed item is
//! isolated; a re-run is idempotent (no double-apply); duplicate item keys dedup at create. Plus the
//! tenancy posture: a run without a company-anchored ambient org scope fails closed before any work.

mod common;
use common::*;

use backbone_bulkops::application::service::bulk_events::LoggingSink;
use backbone_bulkops::application::service::bulk_write_service::*;
use serde_json::json;
use uuid::Uuid;

fn item(k: &str) -> NewItem {
    NewItem { item_key: k.into(), payload: json!({"lead_name": format!("Lead {k}"), "phone": "+628"}) }
}
fn job(items: Vec<NewItem>) -> NewJob {
    NewJob { operation_type: "lead_import".into(), target_module: "crm".into(), submitted_by: None, items }
}

// BGC-1 — a batch runs every item through the target and reports success.
#[tokio::test]
async fn bgc1_run_applies_all() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = BulkWriteService::new(pool.clone());
    let target = FakeTarget::new();
    let sink = CapturingSink::new();

    let j = svc.create_job(job(vec![item("a"), item("b"), item("c")])).await.unwrap();
    let sum = scoped(&pool, company, svc.run_job(j, &target, &sink)).await.unwrap();
    assert_eq!(sum.succeeded, 3);
    assert_eq!(sum.failed, 0);
    assert_eq!(target.apply_count(), 3);

    let status: String = sqlx::query_scalar("SELECT status::text FROM bulkops.bulk_jobs WHERE id=$1")
        .bind(j).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "completed");
    assert_eq!(sink.last().succeeded_count, 3);
}

// BGC-2 — a failed item is isolated: the rest of the batch still applies, the failure is recorded, and the
// job ends 'failed' (completed-with-failures).
#[tokio::test]
async fn bgc2_failed_item_isolated() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = BulkWriteService::new(pool.clone());
    let target = FakeTarget::failing(&["b"]);
    let sink = CapturingSink::new();

    let j = svc.create_job(job(vec![item("a"), item("b"), item("c")])).await.unwrap();
    let sum = scoped(&pool, company, svc.run_job(j, &target, &sink)).await.unwrap();
    assert_eq!(sum.succeeded, 2, "a and c applied despite b failing");
    assert_eq!(sum.failed, 1);
    assert_eq!(target.apply_count(), 2);

    let (status, err): (String, Option<String>) = sqlx::query_as(
        "SELECT status::text, error_detail FROM bulkops.bulk_job_items WHERE job_id=$1 AND item_key='b'")
        .bind(j).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "failed");
    assert_eq!(err.as_deref(), Some("rejected b"));
    let job_status: String = sqlx::query_scalar("SELECT status::text FROM bulkops.bulk_jobs WHERE id=$1")
        .bind(j).fetch_one(&pool).await.unwrap();
    assert_eq!(job_status, "failed", "job ends failed when any item failed");
}

// BGC-3 — a re-run is idempotent: an already-applied item is never re-applied.
#[tokio::test]
async fn bgc3_rerun_idempotent() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = BulkWriteService::new(pool.clone());
    let target = FakeTarget::new();
    let sink = CapturingSink::new();

    let j = svc.create_job(job(vec![item("a"), item("b")])).await.unwrap();
    scoped(&pool, company, svc.run_job(j, &target, &sink)).await.unwrap();
    let second = scoped(&pool, company, svc.run_job(j, &target, &sink)).await.unwrap();
    assert_eq!(second.succeeded, 0, "re-run applies nothing new");
    assert_eq!(target.apply_count(), 2, "each item applied exactly once across two runs");
}

// BGC-4 — duplicate item keys dedup at create (a re-submitted item is collapsed).
#[tokio::test]
async fn bgc4_duplicate_item_key_deduped() {
    let pool = pool().await;
    let svc = BulkWriteService::new(pool.clone());
    let j = svc.create_job(job(vec![item("x"), item("x"), item("y")])).await.unwrap();
    let total: i32 = sqlx::query_scalar("SELECT total_items FROM bulkops.bulk_jobs WHERE id=$1")
        .bind(j).fetch_one(&pool).await.unwrap();
    assert_eq!(total, 2, "duplicate key x collapsed → 2 items");
    let _ = LoggingSink;
}

// BGC-5 — the failure-recovery loop: an operator sees the failures (report) and retries just them
// (completeness council 2026-07-10). Without it a batch with failures is a dead end.
#[tokio::test]
async fn bgc5_report_and_retry_failed() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = BulkWriteService::new(pool.clone());
    let sink = CapturingSink::new();

    let j = svc.create_job(job(vec![item("a"), item("b"), item("c")])).await.unwrap();
    // First run: b fails.
    scoped(&pool, company, svc.run_job(j, &FakeTarget::failing(&["b"]), &sink)).await.unwrap();

    // The operator gets the failure report from the API alone — which key, why.
    let fails = svc.failures(j).await.unwrap();
    assert_eq!(fails.len(), 1);
    assert_eq!(fails[0].item_key, "b");
    assert_eq!(fails[0].error_detail.as_deref(), Some("rejected b"));

    // Fix the cause, retry just the failed item, re-run → job clears.
    let requeued = svc.retry_failed(j).await.unwrap();
    assert_eq!(requeued, 1, "one failed item requeued");
    scoped(&pool, company, svc.run_job(j, &FakeTarget::new(), &sink)).await.unwrap();

    let (status, failed): (String, i32) = sqlx::query_as(
        "SELECT status::text, failed_count FROM bulkops.bulk_jobs WHERE id=$1")
        .bind(j).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "completed", "the batch is finished after the retry");
    assert_eq!(failed, 0);
    assert!(svc.failures(j).await.unwrap().is_empty());
}

// BGC-6 — the tenancy posture (ADR-0029): a run with NO company-anchored ambient org scope fails
// closed before touching the database or the target — the legacy company key the port contract
// requires is never guessed.
#[tokio::test]
async fn bgc6_run_without_company_scope_fails_closed() {
    let pool = pool().await;
    let svc = BulkWriteService::new(pool.clone());
    let target = FakeTarget::new();
    let sink = CapturingSink::new();

    // Created inside a scope; run OUTSIDE any scope.
    let company = Uuid::new_v4();
    let j = scoped(&pool, company, svc.create_job(job(vec![item("a")]))).await.unwrap();

    let res = svc.run_job(j, &target, &sink).await;
    assert!(
        matches!(res, Err(BulkError::NoCompanyScope)),
        "a scopeless run must fail closed with NoCompanyScope, got {res:?}"
    );
    assert_eq!(target.apply_count(), 0, "no item may be applied without a scope");

    // The job was not started: still pending, no outcomes recorded.
    let status: String = sqlx::query_scalar("SELECT status::text FROM bulkops.bulk_jobs WHERE id=$1")
        .bind(j).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "pending", "the job must be untouched by the failed run");
}
