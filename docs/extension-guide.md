# backbone-bulkops — Extension Guide

## Public surface (stable)
- **Target port** (`application::service::bulk_ports`): `BulkTargetPort` + DTOs (`BulkOp`, `BulkAck`,
  `BulkRejected`) — the seam each item drives, implemented over the target module's WRITE PATH (never raw
  SQL). `BulkOp.item_key` MUST be honored as the target's idempotency key.
- **Events** (`application::service::bulk_events`): `BulkJobCompleted`, the `BulkEvent` union, `BulkEventSink`.
- **Write path** (`application::service::bulk_write_service::BulkWriteService`): `create_job`, `run_job`,
  `failures` (the report), `retry_failed` (the recovery loop), with `NewJob`/`NewItem`/`RunSummary`/`FailedItem`.

## How a consuming service (CLI/admin) uses bulk-ops
Implement `BulkTargetPort::apply` over the target module's guarded verb (a lead import → crm `create_lead`,
a price update → selling's price verb), forwarding `item_key` so a re-applied item can't duplicate. Build a
job from the operator's file, `create_job`, then `run_job`. A failed item is recorded with its error; call `failures(job)` for the report,
fix the cause, `retry_failed(job)` to requeue just those items, then `run_job` again.

## Not a contract
- The 12 generated CRUD endpoints per entity are convenience scaffolding. Do **not** flip an item's status
  through the generic PATCH surface — it bypasses the reserve-before-apply guard. Use `BulkWriteService`.
- `// <<< CUSTOM` blocks preserve local edits only; not a cross-module extension point.

## Invariants a consumer must not break
- One item per `(job, item_key)`; each item applied at most once (reserved before the external call); a
  re-run never re-applies an applied item.
- The batch NEVER writes a module's tables directly — it drives the module's write path via the port.
