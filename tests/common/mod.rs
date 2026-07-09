//! Shared test helpers: a live pool, a fake target (records applies / fails chosen keys), a REAL crm
//! target (drives create_lead), and a capturing event sink.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use backbone_bulkops::application::service::bulk_events::{BulkEvent, BulkEventSink};
use backbone_bulkops::application::service::bulk_ports::{BulkAck, BulkOp, BulkRejected, BulkTargetPort};
use sqlx::PgPool;
use uuid::Uuid;

pub fn dburl() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5433/backbone_bulkops".into())
}
pub async fn pool() -> PgPool {
    PgPool::connect(&dburl()).await.expect("connect")
}

/// A fake target: records each applied op, and fails any item whose key is in `fail_keys`.
#[derive(Clone, Default)]
pub struct FakeTarget {
    pub applied: Arc<Mutex<Vec<String>>>,
    pub fail_keys: Arc<Mutex<Vec<String>>>,
}
impl FakeTarget {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn failing(keys: &[&str]) -> Self {
        let f = Self::default();
        *f.fail_keys.lock().unwrap() = keys.iter().map(|s| s.to_string()).collect();
        f
    }
    pub fn apply_count(&self) -> usize {
        self.applied.lock().unwrap().len()
    }
}
#[async_trait::async_trait]
impl BulkTargetPort for FakeTarget {
    async fn apply(&self, op: &BulkOp) -> Result<BulkAck, BulkRejected> {
        if self.fail_keys.lock().unwrap().contains(&op.item_key) {
            return Err(BulkRejected { code: "rejected".into(), message: format!("rejected {}", op.item_key) });
        }
        self.applied.lock().unwrap().push(op.item_key.clone());
        Ok(BulkAck { applied_ref_type: "record".into(), applied_ref_id: Uuid::new_v4() })
    }
}

/// The REAL backbone-crm target: a bulk lead-import item drives crm's create_lead write path.
pub struct RealCrmTarget {
    pub crm: backbone_crm::application::service::crm_write_service::CrmWriteService,
}
impl RealCrmTarget {
    pub fn new(pool: PgPool) -> Self {
        Self { crm: backbone_crm::application::service::crm_write_service::CrmWriteService::new(pool) }
    }
}
#[async_trait::async_trait]
impl BulkTargetPort for RealCrmTarget {
    async fn apply(&self, op: &BulkOp) -> Result<BulkAck, BulkRejected> {
        use backbone_crm::application::service::crm_write_service::NewLead;
        let p = &op.payload;
        let name = p.get("lead_name").and_then(|v| v.as_str()).unwrap_or("Imported lead").to_string();
        let phone = p.get("phone").and_then(|v| v.as_str()).map(|s| s.to_string());
        let id = self.crm.create_lead(NewLead {
            company_id: op.company_id, lead_name: name, organization_name: None,
            phone, whatsapp_no: None, email: None, source: "other".into(), campaign_id: None,
            notes: Some(format!("bulk import {}", op.item_key)),
        }).await.map_err(|e| BulkRejected { code: "crm_rejected".into(), message: e.to_string() })?;
        Ok(BulkAck { applied_ref_type: "lead".into(), applied_ref_id: id })
    }
}

#[derive(Clone, Default)]
pub struct CapturingSink {
    pub events: Arc<Mutex<Vec<BulkEvent>>>,
}
impl CapturingSink {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn last(&self) -> backbone_bulkops::application::service::bulk_events::BulkJobCompleted {
        match self.events.lock().unwrap().last().cloned().expect("an event") {
            BulkEvent::BulkJobCompleted(c) => c,
        }
    }
}
impl BulkEventSink for CapturingSink {
    fn publish(&self, event: &BulkEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
}
