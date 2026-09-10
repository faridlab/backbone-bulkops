//! The target-write-path port (hand-authored, user-owned) — the seam to whatever module a batch drives.
//!
//! Bulk-ops NEVER writes another module's tables. Each item is applied through the target module's own
//! WRITE PATH — a lead import calls crm's `create_lead`, a status change calls the module's guarded verb —
//! so module invariants are never bypassed. Bulk-ops holds only the `BulkTargetPort` trait; a composing
//! service (and the seam test) wires it over the real module. Zero normal Cargo edge.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One operation to apply through the target module's write path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BulkOp {
    /// The legacy company key of the tenant the batch acts for. Tenancy (ADR-0029): this module
    /// carries no scoping column of its own — the key exists here because the TARGET write path is
    /// the sibling module's domain and still keys its rows on the legacy company during the tenancy
    /// transition. The engine sources it from the ambient org scope's company twin and fails the run
    /// closed when the caller carries none; once a target is itself tenant-agnostic its adapter can
    /// ignore this field.
    pub company_id: Uuid,
    pub operation_type: String,
    /// The item's idempotency key within the job — a composing adapter forwards it to the target so a
    /// re-applied item can't create a duplicate.
    pub item_key: String,
    pub payload: serde_json::Value,
}

/// The target accepted the operation and created/changed a record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BulkAck {
    pub applied_ref_type: String,
    pub applied_ref_id: Uuid,
}

/// The target rejected the operation (validation / business-rule failure). `code` is stable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BulkRejected {
    pub code: String,
    pub message: String,
}

/// The write-path seam a batch drives. A composing service implements it over the target module.
///
/// **Idempotency contract (required for exactly-once):** `apply` MUST be idempotent on `(company_id,
/// item_key)` — a second apply of the same op returns the same ref without a second effect. The engine
/// reserves each item (`pending → applying`) before calling `apply`, which stops a CONCURRENT double-apply;
/// but a crash after the target commits and before the mark leaves the item `applying`, and recovery must
/// reconcile it against the target by `item_key`, never blind-re-apply. So the target's `item_key` dedup is
/// the linchpin of exactly-once — the engine guarantees at-least-once + no concurrent duplicate.
///
/// **Re-check contract (required for crash recovery):** `check_applied` answers, for the same
/// `(company_id, item_key)` idempotency key `apply` dedupes on, whether the target already holds the
/// effect. `Ok(Some(ack))` = already applied (return the original ref); `Ok(None)` = the target holds
/// nothing for the key, so a re-apply is safe; `Err` = the target CANNOT DETERMINE it — the engine then
/// cancels the item rather than risk a duplicate effect or a silent drop. A target with no by-key
/// lookup must return `Err`, never guess.
///
/// The `company_id` argument carries the same legacy company twin as [`BulkOp::company_id`] — the
/// target's domain key during the tenancy transition (ADR-0029), fail-closed at the engine when the
/// caller has no company-anchored scope.
#[async_trait::async_trait]
pub trait BulkTargetPort: Send + Sync {
    async fn apply(&self, op: &BulkOp) -> Result<BulkAck, BulkRejected>;

    async fn check_applied(
        &self,
        company_id: Uuid,
        item_key: &str,
    ) -> Result<Option<BulkAck>, BulkRejected>;
}
