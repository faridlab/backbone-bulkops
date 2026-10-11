// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

// The hand-written bulkops SQL's parameter/projection types (see `bulk_job_repository` and
// `bulk_job_item_repository`, both declared `user_owned` in metaphor.codegen.yaml).
pub use bulk_job_item_repository::{FailedItemRow, ItemCountsRow, NewItemRow, PendingItemRow};
pub use bulk_job_repository::{JobRunRow, NewJobRow};
