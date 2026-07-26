//! The write-path seam against the REAL backbone-crm module. A bulk lead-import batch drives crm's
//! `create_lead` for each item — never raw SQL, never bypassing crm's invariants. Proves the batch lands
//! real records through the module's own write path. ZERO normal Cargo edge — crm is reached through the
//! `BulkTargetPort`, a dev-dependency only in the test.

mod common;
use common::*;

use backbone_bulkops::application::service::bulk_write_service::*;
use serde_json::json;
use uuid::Uuid;

// BSEAM-1 — a bulk lead import creates REAL crm leads, one per item, through crm's write path.
#[tokio::test]
async fn bseam1_bulk_import_creates_real_crm_leads() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = BulkWriteService::new(pool.clone());
    let target = RealCrmTarget::new(pool.clone());
    let sink = CapturingSink::new();

    let items = vec![
        NewItem { item_key: format!("k-{}", Uuid::new_v4()), payload: json!({"lead_name": "Budi", "phone": "+628111"}) },
        NewItem { item_key: format!("k-{}", Uuid::new_v4()), payload: json!({"lead_name": "Sari", "phone": "+628222"}) },
    ];
    let j = svc.create_job(NewJob {
        company_id: company, operation_type: "lead_import".into(), target_module: "crm".into(),
        submitted_by: None, items,
    }).await.unwrap();
    let sum = svc.run_job(j, company, &target, &sink).await.unwrap();
    assert_eq!(sum.succeeded, 2);

    // Two REAL crm leads exist for this company, created through create_lead (status 'new', source 'other').
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM crm.leads WHERE company_id=$1 AND status='new'::lead_status")
        .bind(company).fetch_one(&pool).await.unwrap();
    assert_eq!(n, 2, "two real leads imported");

    // Each applied item recorded the created lead id (the audit link back).
    let refs: Vec<(String, Option<Uuid>)> = sqlx::query_as(
        "SELECT applied_ref_type, applied_ref_id FROM bulkops.bulk_job_items WHERE job_id=$1 AND status='applied'::bulk_item_status")
        .bind(j).fetch_all(&pool).await.unwrap();
    assert_eq!(refs.len(), 2);
    for (ty, id) in &refs {
        assert_eq!(ty, "lead");
        let exists: i64 = sqlx::query_scalar("SELECT count(*) FROM crm.leads WHERE id=$1")
            .bind(id.unwrap()).fetch_one(&pool).await.unwrap();
        assert_eq!(exists, 1, "the audit ref points at a real lead");
    }
}
