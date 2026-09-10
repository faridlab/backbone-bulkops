//! Bulk-ops domain events (hand-authored, user-owned) — an audit/observability surface.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A batch finished running (all items terminal).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BulkJobCompleted {
    pub job_id: Uuid,
    /// The legacy company twin of the org scope the run rode (ADR-0029) — routing/observability
    /// data for subscribers whose own rows still key on the legacy company, not a fence of this
    /// module's tables.
    pub company_id: Uuid,
    pub operation_type: String,
    pub total_items: i32,
    pub succeeded_count: i32,
    pub failed_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum BulkEvent {
    BulkJobCompleted(BulkJobCompleted),
}

pub trait BulkEventSink: Send + Sync {
    fn publish(&self, event: &BulkEvent);
}

#[derive(Debug, Default, Clone)]
pub struct LoggingSink;

impl BulkEventSink for LoggingSink {
    fn publish(&self, event: &BulkEvent) {
        tracing::info!(?event, "bulk-ops event");
    }
}
