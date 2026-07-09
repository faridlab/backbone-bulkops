# backbone-bulkops — FSD

## Entities
BulkJob (`company_id`, `operation_type`, `target_module`, `status`, `total_items`, `succeeded_count`,
`failed_count`, `submitted_by?` logical) · BulkJobItem (`job_id` FK, `item_key`, `status`, `payload`,
`applied_ref_type?`/`applied_ref_id?` logical, `error_detail?`; unique `(job_id, item_key)`; index
`(status)`). Enums: BulkJobStatus {pending, running, completed, failed}, BulkItemStatus {pending, applying,
applied, failed}.

## Write path (`BulkWriteService`, hand-authored, user-owned)
- `create_job(NewJob { company_id, operation_type, target_module, submitted_by?, items })` → an audited
  batch of `pending` items (deduped on (job, item_key))
- `run_job(job_id, &dyn BulkTargetPort, &dyn BulkEventSink)` → reserve (`pending → applying`) → apply via
  the target write path → mark `applied`/`failed`; failures isolated; counts roll up from the ledger;
  emits `BulkJobCompleted`; returns `RunSummary {succeeded, failed, skipped}`
- `failures(job_id)` → the failed items with key + error + payload (the report, no private-table read)
- `retry_failed(job_id)` → reset `failed → pending` so `run_job` retries just them; returns the count requeued

Errors: `BulkError {Db, NotFound, Invalid}`.

## Seam (port — zero normal Cargo edge)
- **Apply → target module (proven, BSEAM-1):** each item drives the target module's write path via
  `BulkTargetPort` (proven: REAL backbone-crm `create_lead` for a bulk lead import). `BulkOp.item_key` is
  the idempotency key. Bulk-ops never touches a module's tables.
- **Outbound:** `BulkJobCompleted` for audit.

## Test oracle
`bulk_golden_cases` (5: BGC-1 run applies all, BGC-2 failed item isolated, BGC-3 re-run idempotent, BGC-4
duplicate item key deduped, BGC-5 report + retry failed),
`integrity_probes` (4: BIP-1 job needs items, BIP-2 item needs key, BIP-3 counts roll up from the ledger,
BIP-4 concurrent runs apply once),
`bulk_crm_seam` (1: BSEAM-1 bulk import creates REAL crm leads through create_lead) + §5 round-trip.
**10 tests.**

> The generated `integration_tests.rs` hits an external HTTP server and is environmental scaffolding, not
> part of this module's correctness gate.
