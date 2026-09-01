//! Tenant-isolation probe (maturity council 2026-08-12, Recommendation #1).
//!
//! The engine's per-item SQL is ID-only — every `WHERE` is `id=$1` or `job_id=$1`, with NO
//! `company_id` predicate. Tenant isolation is therefore 100% dependent on PostgreSQL row-level
//! security firing under a NON-superuser connection role that has `app.company_id` set.
//!
//! Every other test in this suite connects as the `postgres` SUPERUSER
//! (`tests/common/mod.rs:13-16`), which BYPASSES RLS entirely (`ENABLE`/`FORCE ROW LEVEL SECURITY`
//! are no-ops for a superuser/BYPASSRLS role). Those tests cannot distinguish "RLS works" from
//! "RLS is silently inert." This file closes that gap: it connects a second pool as a
//! `NOBYPASSRLS` non-superuser role and asserts the fence under the privilege level a real
//! deployment would actually use.
//!
//! Two cases, so a RED is unambiguous:
//! - TIS-1 (control): a tenant running its OWN job succeeds end-to-end → proves the non-superuser
//!   role + `company_scope` propagation work for the happy path (so a TIS-2 failure isn't a
//!   harness/privilege artifact).
//! - TIS-2 (the probe): tenant B running tenant A's job is fenced → `NotFound`, zero applies,
//!   A's items untouched.
//!
//! If TIS-1 is GREEN and TIS-2 is RED, the RLS fence has a hole. If both are RED, the role is
//! mis-granted. If both are GREEN, the headline isolation claim is finally *verified*, not merely
//! asserted by DDL + comments.
//!
//! Requires a live Postgres at `DATABASE_URL` (default `localhost:5433/backbone_bulkops`) that the
//! `postgres` superuser can reach — the probe creates the `bulkops_tenant` role itself.

mod common;
use common::*;

use backbone_bulkops::application::service::bulk_write_service::*;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

/// Connection string for the non-superuser tenant role. Override via `DATABASE_URL_TENANT`; defaults
/// to the same host/db as the superuser pool but authenticated as `bulkops_tenant`.
fn tenant_dburl() -> String {
    std::env::var("DATABASE_URL_TENANT")
        .unwrap_or_else(|_| "postgres://bulkops_tenant:bulkops_tenant@localhost:5433/backbone_bulkops".into())
}

/// Serializes [`ensure_tenant_role`] across the parallel test threads: concurrent catalog writes
/// (CREATE ROLE racing itself, or two GRANTs on the same schema updating one catalog tuple) fail
/// with raw XX000 errors that say nothing about the fence under test.
static ROLE_SETUP_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A `NOBYPASSRLS` LOGIN role with exactly the privileges a deployed app role needs: USAGE on the
/// `bulkops` (+ `public`, where the enum types live) schemas, CRUD on the two tables, and USAGE on
/// the `bulk_job_status` / `bulk_item_status` enum types. Idempotent — safe to run before every test.
#[expect(clippy::expect_used, reason = "test harness: a panic here names the setup failure precisely")]
async fn ensure_tenant_role(admin: &PgPool) {
    let _guard = ROLE_SETUP_LOCK.lock().await;
    // CREATE ROLE if missing. NOBYPASSRLS is the default for non-superusers, but state it explicitly
    // so the test's meaning doesn't depend on a server default. The plain CREATE (not an IF NOT
    // EXISTS dance) tolerates the parallel-test race: two threads may both see the role missing and
    // both create — one wins, the loser's duplicate-key error is the expected outcome.
    if let Err(e) = sqlx::query("CREATE ROLE bulkops_tenant LOGIN PASSWORD 'bulkops_tenant' NOBYPASSRLS")
        .execute(admin)
        .await
    {
        let msg = e.to_string();
        assert!(
            msg.contains("already exists") || msg.contains("pg_authid_rolname_index"),
            "unexpected error creating bulkops_tenant: {e}"
        );
    }

    sqlx::query("GRANT USAGE ON SCHEMA bulkops TO bulkops_tenant")        .execute(admin).await.expect("grant schema bulkops");
    // The enum types are created UNQUALIFIED in the first migration (before CREATE SCHEMA bulkops),
    // so they live in `public` — the role needs USAGE on `public` to resolve them.
    sqlx::query("GRANT USAGE ON SCHEMA public TO bulkops_tenant")        .execute(admin).await.expect("grant schema public");
    sqlx::query("GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA bulkops TO bulkops_tenant")        .execute(admin).await.expect("grant table crud");
    // USAGE on the enum types wherever they actually live (public, by the migration ordering above).
    sqlx::query(
        r#"DO $$
        DECLARE t oid;
        BEGIN
          FOR t IN SELECT oid FROM pg_type WHERE typname IN ('bulk_job_status', 'bulk_item_status') LOOP
            EXECUTE format('GRANT USAGE ON TYPE %s TO bulkops_tenant', t::regtype);
          END LOOP;
        END $$"#,
    )
    .execute(admin)
    .await    .expect("grant enum type usage");
}

/// A pool connected as the non-superuser tenant role — the privilege level a real deployment uses,
/// and the only level at which RLS actually fires.
#[expect(clippy::expect_used, reason = "test harness: a panic here names the setup failure precisely")]
async fn tenant_pool() -> PgPool {    PgPool::connect(&tenant_dburl()).await.expect("connect as bulkops_tenant")
}

fn item(k: &str) -> NewItem {
    NewItem { item_key: k.into(), payload: json!({"lead_name": format!("Lead {k}"), "phone": "+628"}) }
}
fn job(company: Uuid, items: Vec<NewItem>) -> NewJob {
    NewJob { company_id: company, operation_type: "lead_import".into(), target_module: "crm".into(), submitted_by: None, items }
}

// TIS-1 (control) — a tenant running its OWN job succeeds end-to-end under the non-superuser role.
// Proves the role grants + `company_scope` propagation are correct, so a TIS-2 RED is a real fence
// hole, not a broken harness.
#[tokio::test]
#[expect(clippy::expect_used, reason = "test harness: a panic here names the setup failure precisely")]
async fn tis1_own_tenant_run_succeeds() {
    let admin = pool().await; // superuser — role setup only
    ensure_tenant_role(&admin).await;
    let tenant = tenant_pool().await; // NON-superuser — RLS fires here

    let company_a = Uuid::new_v4();
    // Create the job as the superuser (RLS bypassed) so setup can't be confounded by create-path
    // privilege edges; the property under test is the RUN-path fence.
    let job_a = BulkWriteService::new(admin.clone())
        .create_job(job(company_a, vec![item("a1"), item("a2")]))
        .await        .expect("create company-A job");

    let target = FakeTarget::new();
    let sink = CapturingSink::new();
    // Same role, but `run_job` scopes the connection to company_a → RLS lets A see and run A's job.
    let sum = BulkWriteService::new(tenant.clone())
        .run_job(job_a, company_a, &target, &sink)
        .await        .expect("own-tenant run must succeed under the non-superuser role");

    assert_eq!(sum.succeeded, 2, "control: own-tenant run applies both items");
    assert_eq!(target.apply_count(), 2);

    let status: String = sqlx::query_scalar("SELECT status::text FROM bulkops.bulk_jobs WHERE id=$1")
        .bind(job_a).fetch_one(&admin).await.unwrap();
    assert_eq!(status, "completed", "control: the job completes for the owning tenant");
}

// TIS-2 (the probe) — tenant B running tenant A's job is fenced. This is the case the superuser
// suite could never exercise: under a non-superuser role, RLS must hide A's job from a B-scoped run.
#[tokio::test]
#[expect(clippy::expect_used, reason = "test harness: a panic here names the setup failure precisely")]
async fn tis2_cross_tenant_run_is_fenced() {
    let admin = pool().await;
    ensure_tenant_role(&admin).await;
    let tenant = tenant_pool().await;

    let company_a = Uuid::new_v4();
    let company_b = Uuid::new_v4(); // a different tenant
    let job_a = BulkWriteService::new(admin.clone())
        .create_job(job(company_a, vec![item("a1"), item("a2")]))
        .await        .expect("create company-A job");

    let target = FakeTarget::new();
    let sink = CapturingSink::new();
    // The probe: run A's job scoped to B. A correct fence makes A's job invisible (NotFound) and
    // prevents any apply — indistinguishable from a missing job, so no id-existence leak either.
    let res = BulkWriteService::new(tenant.clone())
        .run_job(job_a, company_b, &target, &sink)
        .await;

    assert!(
        matches!(res, Err(BulkError::NotFound("job"))),
        "cross-tenant run must be fenced → NotFound, got {res:?}"
    );
    assert_eq!(target.apply_count(), 0, "no item may be applied under a mismatched tenant");

    // A's items must be untouched (still pending) and the job must NOT have been marked running by B.
    let item_statuses: Vec<String> =
        sqlx::query_scalar("SELECT status::text FROM bulkops.bulk_job_items WHERE job_id=$1 ORDER BY item_key")
            .bind(job_a)
            .fetch_all(&admin)
            .await
            .unwrap();
    assert!(
        item_statuses.iter().all(|s| s == "pending"),
        "company-A items must be unchanged by the cross-tenant run; got {item_statuses:?}"
    );
    let job_status: String = sqlx::query_scalar("SELECT status::text FROM bulkops.bulk_jobs WHERE id=$1")
        .bind(job_a).fetch_one(&admin).await.unwrap();
    assert_eq!(job_status, "pending", "the job must not be mutated by a mismatched tenant");

    let _ = sink; // sink captured no BulkJobCompleted; nothing to assert beyond apply_count above.
}
