//! Integrity probes — the batch invariants: a job needs items, an item needs a key, and the job's counts
//! roll up from the item ledger (authoritative across runs) — including under two concurrent runs.

mod common;
use common::*;

use backbone_bulkops::application::service::bulk_write_service::*;
use serde_json::json;
use uuid::Uuid;

fn item(k: &str) -> NewItem {
    NewItem { item_key: k.into(), payload: json!({"lead_name": k}) }
}

// BIP-1 — a job needs at least one item.
#[tokio::test]
async fn bip1_job_needs_items() {
    let pool = pool().await;
    let svc = BulkWriteService::new(pool.clone());
    let r = svc.create_job(NewJob {
        operation_type: "x".into(), target_module: "crm".into(),
        submitted_by: None, items: vec![],
    }).await;
    assert!(matches!(r, Err(BulkError::Invalid(_))));
}

// BIP-2 — an item needs a non-blank key.
#[tokio::test]
async fn bip2_item_needs_key() {
    let pool = pool().await;
    let svc = BulkWriteService::new(pool.clone());
    let r = svc.create_job(NewJob {
        operation_type: "x".into(), target_module: "crm".into(),
        submitted_by: None, items: vec![NewItem { item_key: "  ".into(), payload: json!({}) }],
    }).await;
    assert!(matches!(r, Err(BulkError::Invalid(_))));
}

// BIP-3 — the job counts roll up from the item ledger (a partial re-run doesn't lose prior successes).
#[tokio::test]
async fn bip3_counts_rollup_from_ledger() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = BulkWriteService::new(pool.clone());
    let j = svc.create_job(NewJob {
        operation_type: "lead_import".into(), target_module: "crm".into(),
        submitted_by: None, items: vec![item("a"), item("b"), item("c")],
    }).await.unwrap();

    // First run fails b; second run (b still fails) — counts reflect the full ledger, not just this run.
    scoped(&pool, company, svc.run_job(j, &FakeTarget::failing(&["b"]), &CapturingSink::new())).await.unwrap();
    let sink = CapturingSink::new();
    scoped(&pool, company, svc.run_job(j, &FakeTarget::failing(&["b"]), &sink)).await.unwrap();

    let (succ, fail): (i32, i32) = sqlx::query_as(
        "SELECT succeeded_count, failed_count FROM bulkops.bulk_jobs WHERE id=$1")
        .bind(j).fetch_one(&pool).await.unwrap();
    assert_eq!(succ, 2, "a + c applied (counted once, across runs)");
    assert_eq!(fail, 1, "b failed");
}

// BIP-4 — TWO concurrent runs apply each item at most once (maturity council 2026-07-10). The engine
// reserves each item (pending→applying) BEFORE the external apply, so a concurrent runner that lost the
// reservation skips it — the target is never double-applied (which the audit would otherwise conceal).
#[tokio::test]
async fn bip4_concurrent_runs_apply_once() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = BulkWriteService::new(pool.clone());
    let target = FakeTarget::new();
    let j = svc.create_job(NewJob {
        operation_type: "lead_import".into(), target_module: "crm".into(),
        submitted_by: None,
        items: (0..8).map(|i| item(&format!("i{i}"))).collect(),
    }).await.unwrap();

    // Two runners race the same job on the same pool, each in its own org-scoped session.
    let (svc1, svc2) = (BulkWriteService::new(pool.clone()), BulkWriteService::new(pool.clone()));
    let (t1, t2) = (target.clone(), target.clone());
    let (p1, p2) = (pool.clone(), pool.clone());
    let (r1, r2) = tokio::join!(
        async move { scoped(&p1, company, svc1.run_job(j, &t1, &CapturingSink::new())).await },
        async move { scoped(&p2, company, svc2.run_job(j, &t2, &CapturingSink::new())).await },
    );
    r1.unwrap();
    r2.unwrap();

    // Every item was applied to the target EXACTLY once — not twice, despite two concurrent runs.
    assert_eq!(target.apply_count(), 8, "each of 8 items applied exactly once across two concurrent runs");
    let applied: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bulkops.bulk_job_items WHERE job_id=$1 AND status='applied'::bulk_item_status")
        .bind(j).fetch_one(&pool).await.unwrap();
    assert_eq!(applied, 8);
}
