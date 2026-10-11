//! The extension of `services.rs` (hand-written; ADR-0031).

#[allow(unused_imports)]
use super::*;

/// The batch contract surface for composing services — everything a host or adapter needs to drive
/// batches WITHOUT importing the module's internals.
///
/// A composing service:
/// 1. implements [`BulkTargetPort`] over the target module's real write path (this is the ONLY seam —
///    bulk-ops never writes another module's tables, and there is no normal Cargo edge to one);
/// 2. drives batches through the [`BulkWriteService`] handle — `create_job`, then `run_job` (and, on a
///    crashed run, `reconcile_applying`), plus `failures` / `retry_failed` for the operator loop;
/// 3. subscribes to completion through a [`BulkEventSink`] it passes in (mapping persistence gates on
///    `BulkJobCompleted`).
pub use crate::application::service::bulk_ports::{BulkAck, BulkOp, BulkRejected, BulkTargetPort};
pub use crate::application::service::bulk_write_service::{
    BulkError, BulkWriteService, FailedItem, NewItem, NewJob, ReconcileSummary, RunSummary,
};
pub use crate::application::service::bulk_events::{
    BulkEvent, BulkEventSink, BulkJobCompleted, LoggingSink,
};
