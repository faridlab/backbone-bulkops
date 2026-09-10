//! Reconcile probes — the crashed-run recovery contract: a stranded `applying` item is re-checked
//! against the target by `item_key`, never blind-re-applied. The three exits (confirmed / re-applied /
//! cancelled) and the age fence, plus the job roll-up that never masks a cancellation.

mod common;
use common::*;

use backbone_bulkops::application::service::bulk_events::BulkEvent;
use backbone_bulkops::application::service::bulk_write_service::*;
use serde_json::json;
use uuid::Uuid;

fn item(k: &str) -> NewItem {
    NewItem { item_key: k.into(), payload: json!({"lead_name": k}) }
}

/// Simulate the crash residue: the runner reserved the item (CAS `pending → applying`) and died before
/// the outcome mark. Backdate the audit `updated_at` so the claim reads older than `older_than`.
/// The update runs with triggers sidelined (`session_replication_role = replica`) because the audit
/// trigger would otherwise stamp `updated_at = now()` over the backdate; the scratch connection is a
/// superuser, which is exactly what that escape hatch requires — test fixture only.
async fn strand(pool: &sqlx::PgPool, job_id: Uuid, key: &str) {
    // All three statements must run on ONE pooled connection — a pool may hand each query a
    // different backend, which would strand the SET on a connection the UPDATE never sees.
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("SET session_replication_role = replica")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        r#"UPDATE bulkops.bulk_job_items
           SET status='applying'::bulk_item_status,
               metadata = jsonb_set(metadata, '{updated_at}', '"2026-01-01T00:00:00Z"'::jsonb)
           WHERE job_id=$1 AND item_key=$2"#,
    )
    .bind(job_id)
    .bind(key)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query("SET session_replication_role = DEFAULT")
        .execute(&mut *conn)
        .await
        .unwrap();
}

// BRP-1 — the target already holds the effect: reconcile CONFIRMS it (marks applied with the target's
// ref) and does NOT re-apply. A non-terminal sibling keeps the job open (no roll-up, no event).
#[tokio::test]
async fn brp1_confirmed_without_reapply() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = BulkWriteService::new(pool.clone());
    let target = FakeTarget::new();
    target.preseed_applied(&["a"]); // the crashed runner's apply landed in the target
    let sink = CapturingSink::new();

    let j = svc.create_job(NewJob {
        operation_type: "lead_import".into(), target_module: "lead".into(),
        submitted_by: None, items: vec![item("a"), item("b")],
    }).await.unwrap();
    strand(&pool, j, "a").await;

    let sum = scoped(&pool, company, svc.reconcile_applying(j, &target, &sink, chrono::Duration::minutes(5))).await.unwrap();
    assert_eq!(sum.confirmed, 1, "a confirmed from the target's own ledger");
    assert_eq!(sum.reapplied, 0);
    assert_eq!(sum.cancelled, 0);
    assert_eq!(target.apply_count(), 1, "the preseeded effect is the ONLY apply — no re-apply");
    assert_eq!(sum.unfinished, 1, "b is still pending");

    let (status, ref_id): (String, Option<Uuid>) = sqlx::query_as(
        "SELECT status::text, applied_ref_id FROM bulkops.bulk_job_items WHERE job_id=$1 AND item_key='a'")
        .bind(j).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "applied");
    assert!(ref_id.is_some(), "the target's original ref was recorded");

    // The job is NOT terminal (b pending) — no roll-up happened.
    let job_status: String = sqlx::query_scalar("SELECT status::text FROM bulkops.bulk_jobs WHERE id=$1")
        .bind(j).fetch_one(&pool).await.unwrap();
    assert_eq!(job_status, "pending");
    assert!(sink.events.lock().unwrap().is_empty(), "no completion event while work remains");
}

// BRP-2 — the target holds nothing for the key: reconcile re-applies (safe — apply is idempotent on
// (company_id, item_key)), the item records that outcome, and the emptied job rolls up + emits
// BulkJobCompleted.
#[tokio::test]
async fn brp2_missing_effect_is_reapplied() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = BulkWriteService::new(pool.clone());
    let target = FakeTarget::new(); // holds nothing — the crash beat the target's commit
    let sink = CapturingSink::new();

    let j = svc.create_job(NewJob {
        operation_type: "lead_import".into(), target_module: "lead".into(),
        submitted_by: None, items: vec![item("c")],
    }).await.unwrap();
    strand(&pool, j, "c").await;

    let sum = scoped(&pool, company, svc.reconcile_applying(j, &target, &sink, chrono::Duration::minutes(5))).await.unwrap();
    assert_eq!(sum.reapplied, 1);
    assert_eq!(sum.confirmed, 0);
    assert_eq!(sum.cancelled, 0);
    assert_eq!(sum.unfinished, 0);
    assert_eq!(target.apply_count(), 1, "the item was re-applied exactly once");

    let status: String = sqlx::query_scalar(
        "SELECT status::text FROM bulkops.bulk_job_items WHERE job_id=$1 AND item_key='c'")
        .bind(j).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "applied");

    let job_status: String = sqlx::query_scalar("SELECT status::text FROM bulkops.bulk_jobs WHERE id=$1")
        .bind(j).fetch_one(&pool).await.unwrap();
    assert_eq!(job_status, "completed", "the emptied job rolled up");

    let last = sink.last();
    assert_eq!(last.job_id, j);
    assert_eq!(last.succeeded_count, 1);
    assert_eq!(last.failed_count, 0, "mapping persistence may gate on this event");
}

// BRP-3 — the target cannot determine the item: it exits to terminal `cancelled` (never a guess in
// either direction), and the job's terminal status is `cancelled`, not completed/failed.
#[tokio::test]
async fn brp3_unverifiable_item_cancels() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = BulkWriteService::new(pool.clone());
    let target = FakeTarget::opaque(&["d"]); // check_applied errors for d
    let sink = CapturingSink::new();

    let j = svc.create_job(NewJob {
        operation_type: "lead_import".into(), target_module: "lead".into(),
        submitted_by: None, items: vec![item("d")],
    }).await.unwrap();
    strand(&pool, j, "d").await;

    let sum = scoped(&pool, company, svc.reconcile_applying(j, &target, &sink, chrono::Duration::minutes(5))).await.unwrap();
    assert_eq!(sum.cancelled, 1);
    assert_eq!(sum.confirmed, 0);
    assert_eq!(sum.reapplied, 0);
    assert_eq!(sum.unfinished, 0);
    assert_eq!(target.apply_count(), 0, "an unverifiable item is never re-applied");

    let (status, error): (String, Option<String>) = sqlx::query_as(
        "SELECT status::text, error_detail FROM bulkops.bulk_job_items WHERE job_id=$1 AND item_key='d'")
        .bind(j).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "cancelled", "cancelled is terminal");
    assert!(error.as_deref().unwrap_or("").contains("unverifiable"), "the reason is recorded");

    let job_status: String = sqlx::query_scalar("SELECT status::text FROM bulkops.bulk_jobs WHERE id=$1")
        .bind(j).fetch_one(&pool).await.unwrap();
    assert_eq!(job_status, "cancelled", "the job surfaces its cancelled item");

    let last_event = sink.events.lock().unwrap().last().cloned();
    match last_event {
        Some(BulkEvent::BulkJobCompleted(c)) => {
            assert_eq!(c.succeeded_count, 0);
            assert_eq!(c.failed_count, 0, "cancelled is neither success nor failure");
        }
        other => panic!("expected a completion event, got {:?}", other),
    }
}

// BRP-4 — the age fence: a FRESH applying claim (a live runner may still be inside the target) is not
// touched by a pass whose older_than it does not meet.
#[tokio::test]
async fn brp4_fresh_claim_untouched() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = BulkWriteService::new(pool.clone());
    let target = FakeTarget::new();
    let sink = CapturingSink::new();

    let j = svc.create_job(NewJob {
        operation_type: "lead_import".into(), target_module: "lead".into(),
        submitted_by: None, items: vec![item("e")],
    }).await.unwrap();
    // Reserve without backdating: the claim is brand new, so older_than=1h must not match it.
    sqlx::query(
        "UPDATE bulkops.bulk_job_items SET status='applying'::bulk_item_status WHERE job_id=$1 AND item_key='e'",
    )
    .bind(j).execute(&pool).await.unwrap();

    let sum = scoped(&pool, company, svc.reconcile_applying(j, &target, &sink, chrono::Duration::hours(1))).await.unwrap();
    assert_eq!(sum.confirmed + sum.reapplied + sum.cancelled, 0, "nothing was reconciled");
    assert_eq!(sum.unfinished, 1, "the live claim still stands");

    let status: String = sqlx::query_scalar(
        "SELECT status::text FROM bulkops.bulk_job_items WHERE job_id=$1 AND item_key='e'")
        .bind(j).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "applying", "a live runner's claim is respected");
}

// BRP-5 — a later run never masks a cancellation: after reconcile cancels one item, run_job applies
// the remaining pending item, and the job's roll-up still reads `cancelled` (with the applied count
// carried), so the operator cannot miss the unverifiable item.
#[tokio::test]
async fn brp5_run_does_not_mask_cancellation() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = BulkWriteService::new(pool.clone());
    let target = FakeTarget::opaque(&["f"]);
    let sink = CapturingSink::new();

    let j = svc.create_job(NewJob {
        operation_type: "lead_import".into(), target_module: "lead".into(),
        submitted_by: None, items: vec![item("f"), item("g")],
    }).await.unwrap();
    strand(&pool, j, "f").await;

    let sum = scoped(&pool, company, svc.reconcile_applying(j, &target, &sink, chrono::Duration::minutes(5))).await.unwrap();
    assert_eq!(sum.cancelled, 1);
    assert_eq!(sum.unfinished, 1, "g still pending");

    // The operator runs the rest of the batch.
    let run = scoped(&pool, company, svc.run_job(j, &FakeTarget::new(), &sink)).await.unwrap();
    assert_eq!(run.succeeded, 1);

    let (job_status, succeeded): (String, i32) = sqlx::query_as(
        "SELECT status::text, succeeded_count FROM bulkops.bulk_jobs WHERE id=$1")
        .bind(j).fetch_one(&pool).await.unwrap();
    assert_eq!(job_status, "cancelled", "the cancelled item keeps the job marked cancelled");
    assert_eq!(succeeded, 1, "the applied item is still counted");
}
