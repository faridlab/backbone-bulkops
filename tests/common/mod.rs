//! Shared test helpers: a live pool, a fake target (records applies / fails chosen keys / answers
//! re-checks from its own ledger), a REAL lead-module target (drives create_lead), and a capturing
//! event sink.

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
#[expect(clippy::expect_used, reason = "test harness: a panic here names the setup failure precisely")]
pub async fn pool() -> PgPool {
    PgPool::connect(&dburl()).await.expect("connect")
}

/// A fake target: keeps a key→ack ledger of each applied op, fails any item whose key is in
/// `fail_keys`, and answers re-checks from that ledger. Set `opaque_keys` to make the re-check itself
/// fail for chosen keys (a target that cannot answer — the reconcile-to-cancelled path).
#[derive(Clone, Default)]
pub struct FakeTarget {
    pub applied: Arc<Mutex<Vec<(String, BulkAck)>>>,
    pub fail_keys: Arc<Mutex<Vec<String>>>,
    pub opaque_keys: Arc<Mutex<Vec<String>>>,
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
    pub fn opaque(keys: &[&str]) -> Self {
        let f = Self::default();
        *f.opaque_keys.lock().unwrap() = keys.iter().map(|s| s.to_string()).collect();
        f
    }
    pub fn apply_count(&self) -> usize {
        self.applied.lock().unwrap().len()
    }
    /// Pretend the target already applied `keys` BEFORE the engine ran (a crash-between-commit-and-mark
    /// leaves the item `applying` while the target holds the effect) — seeds the key→ack ledger.
    pub fn preseed_applied(&self, keys: &[&str]) {
        for k in keys {
            self.applied.lock().unwrap().push((
                (*k).to_string(),
                BulkAck { applied_ref_type: "record".into(), applied_ref_id: Uuid::new_v4() },
            ));
        }
    }
}
#[async_trait::async_trait]
impl BulkTargetPort for FakeTarget {
    async fn apply(&self, op: &BulkOp) -> Result<BulkAck, BulkRejected> {
        if self.fail_keys.lock().unwrap().contains(&op.item_key) {
            return Err(BulkRejected { code: "rejected".into(), message: format!("rejected {}", op.item_key) });
        }
        let mut ledger = self.applied.lock().unwrap();
        if let Some((_, ack)) = ledger.iter().find(|(k, _)| *k == op.item_key) {
            return Ok(ack.clone()); // idempotent on (company_id, item_key), as the port contract requires
        }
        let ack = BulkAck { applied_ref_type: "record".into(), applied_ref_id: Uuid::new_v4() };
        ledger.push((op.item_key.clone(), ack.clone()));
        Ok(ack)
    }
    async fn check_applied(&self, _company_id: Uuid, item_key: &str) -> Result<Option<BulkAck>, BulkRejected> {
        if self.opaque_keys.lock().unwrap().contains(&item_key.to_string()) {
            return Err(BulkRejected {
                code: "unverifiable".into(),
                message: format!("cannot determine whether {} was applied", item_key),
            });
        }
        Ok(self.applied.lock().unwrap().iter().find(|(k, _)| k == item_key).map(|(_, a)| a.clone()))
    }
}

/// The REAL backbone-lead target: a bulk lead-import item drives the lead module's create_lead write path.
pub struct RealLeadTarget {
    pub leads: backbone_lead::application::service::lead_write_service::LeadWriteService,
}
impl RealLeadTarget {
    pub fn new(pool: PgPool) -> Self {
        Self { leads: backbone_lead::application::service::lead_write_service::LeadWriteService::new(pool) }
    }
}
#[async_trait::async_trait]
impl BulkTargetPort for RealLeadTarget {
    async fn apply(&self, op: &BulkOp) -> Result<BulkAck, BulkRejected> {
        use backbone_lead::application::service::lead_write_service::NewLead;
        let p = &op.payload;
        let name = p.get("lead_name").and_then(|v| v.as_str()).unwrap_or("Imported lead").to_string();
        let phone = p.get("phone").and_then(|v| v.as_str()).map(|s| s.to_string());
        let id = self.leads.create_lead(NewLead {
            company_id: op.company_id, lead_name: name, organization_name: None,
            phone, whatsapp_no: None, email: None, source: "other".into(), campaign_id: None,
            notes: Some(format!("bulk import {}", op.item_key)),
            owner_user_id: None, sales_team_id: None,
            utm_source: None, utm_medium: None, utm_campaign: None,
        }).await.map_err(|e| BulkRejected { code: "lead_rejected".into(), message: e.to_string() })?;
        Ok(BulkAck { applied_ref_type: "lead".into(), applied_ref_id: id })
    }
    async fn check_applied(&self, _company_id: Uuid, _item_key: &str) -> Result<Option<BulkAck>, BulkRejected> {
        // The lead write path has no by-import-key lookup, so the real seam cannot answer a re-check;
        // returning unverifiable exercises the same reconcile-to-cancelled path a real target takes
        // when it cannot confirm its own history.
        Err(BulkRejected { code: "unverifiable".into(), message: "the lead write path has no item-key lookup".into() })
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
    #[expect(clippy::expect_used, reason = "test harness: a panic here names the setup failure precisely")]
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
