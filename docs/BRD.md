# backbone-bulkops — BRD

## Documents
BulkJob (an audited batch) · BulkJobItem (one operation, unique per (job, item_key)). Own Postgres schema
`bulkops`. Posts **no GL**. Drives target modules' write paths through a port.

## Business rules

**BR-1 (create).** `create_job` records a batch of `pending` items, deduped on **(job, item_key)** — a
duplicate key within the batch is collapsed; `total_items` reflects what was actually inserted. A job needs
an `operation_type` and ≥1 item; each item needs a non-blank `item_key`.

**BR-2 (run — the exactly-once invariant).** `run_job` applies each **pending** item through the
`BulkTargetPort` (the target module's write path — never raw SQL, never bypassing invariants). Each item is
**RESERVED first** (`pending → applying`, a CAS) BEFORE the external apply, so a concurrent runner that loses
the reservation skips it — the target is applied **at most once** (no concurrent double-apply). On success
the item is `applied` with the created record's ref (the audit link); on rejection it is `failed` with the
error. A **failed item is isolated** — recorded, the batch continues.

**BR-3 (idempotent re-run).** A re-run processes only still-`pending` items, so an already-applied item is
never re-applied. The job's `succeeded_count`/`failed_count` roll up from the item ledger (authoritative
across runs). The job ends `completed` (all applied) or `failed` (≥1 failed).

**BR-4 (failure recovery).** `failures(job)` returns the failed items (key + error + payload) so the
operator sees what/why without querying the private ledger; `retry_failed(job)` resets `failed → pending`
so `run_job` retries just them after the cause is fixed (completeness council 2026-07-10).

## Events
`BulkJobCompleted` (job_id, company/operation_type, total_items, succeeded_count, failed_count).

## The exactly-once contract
The `BulkTargetPort` MUST be idempotent on `(company_id, item_key)`. The engine guarantees at-least-once +
no concurrent duplicate; crash-gap exactly-once is the target's `item_key` dedup (documented on the trait).

## Deferred (with reason)
Concrete target adapters (wired by the CLI/admin service), a scheduling/queue engine, intra-batch
parallelism, a failed-item reaper (tier5-deferred §6).
