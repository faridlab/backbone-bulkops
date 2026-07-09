# backbone-bulkops — PRD

Operations-adjacents (Tier 5) · cross-cutting **batch-ops utility** · posts no GL.

## Why
An operator drowning in per-record edits needs **mass back-office operations** — bulk import, bulk status
change, bulk price update — done **safely**: audited (who changed what, and the outcome per record),
**idempotent** (a re-submitted or re-run batch never doubles a record), and **never bypassing module
invariants** (each item goes through the target module's own write path, not raw SQL). Promoted for an
operator with high-volume edits (tier5-deferred §6). Likely wired into the CLI/admin service.

## Scope (KEEP — tier5-deferred.md §6)
- **BulkJob** — an audited batch: operation type, target module, status, and the succeeded/failed counts.
- **BulkJobItem** — one operation in the batch, unique per `(job, item_key)`, with its outcome
  (`applied`/`failed` + error) and the record it created/changed (the audit link).
- **The batch engine** — `create_job` records the items (deduped on `(job, item_key)`); `run_job` applies
  each **pending** item through a `BulkTargetPort` (the target module's write path), records the outcome,
  and rolls up the counts. **Idempotent** (a re-run processes only still-pending items); **partial-failure
  isolated** (a failed item is recorded, the batch continues).

## Non-goals (CUT / DEFER — tier5-deferred.md §6)
- The concrete target adapters (a lead-import adapter over crm, a price-update adapter over selling) — the
  composing CLI/admin service wires them behind `BulkTargetPort`.
- A scheduling/queue engine — `run_job` is driven synchronously by the operator/CLI.
- Row-level parallelism / sharding of a single batch.

## Success criteria
- Each item is applied **at most once** through the target write path (idempotent per `(job, item_key)`),
  and a re-run never doubles a record.
- A failed item is isolated (recorded, the batch continues); the job's counts are authoritative across runs.
- A bulk import lands real records through a module's own write path (proven against REAL backbone-crm).
- Zero normal Cargo edge; survives a full codegen regen (§5).
