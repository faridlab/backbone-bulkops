# backbone-bulkops — business flows & golden cases

## Flow: create → run (reserve → apply → mark)
```
create_job (items, dedup on (job, item_key)) → pending items
   │
   ▼  run_job → per PENDING item:
   │     reserve (pending → applying, CAS) — a concurrent runner that loses this skips the item
   │       └─ apply via BulkTargetPort (the target module's write path) → applied(+ref) | failed(+error)
   │     (a failed item is isolated — recorded, the batch continues)
   │
   ▼  counts roll up from the item ledger → job completed | failed → BulkJobCompleted
```
A re-run advances only still-pending items (idempotent). Posts NO GL; drives modules' write paths only.

## Golden cases (`tests/bulk_golden_cases.rs`)
- **BGC-1 — run applies all.** 3 items → 3 applied, job `completed`.
- **BGC-2 — failed item isolated.** Item b fails; a + c still apply; b recorded `failed` + error; job `failed`.
- **BGC-3 — re-run idempotent.** A second run applies nothing new; each item applied once.
- **BGC-4 — duplicate item key deduped.** Two items with key x → collapsed to one at create.
- **BGC-5 — report + retry failed.** A failed item shows in `failures(job)` with its error; `retry_failed`
  requeues it; a re-run clears the job to `completed`. Proven-by-revert.

## Integrity probes (`tests/integrity_probes.rs`)
- **BIP-1 — job needs items.**
- **BIP-2 — item needs a key.**
- **BIP-3 — counts roll up from the ledger.** Across two runs, applied counted once.
- **BIP-4 — concurrent runs apply once.** Two concurrent `run_job` on an 8-item job → each applied exactly
  once (not 16) — the reserve-before-apply closes the concurrent double-apply. Proven-by-revert.

## Seam (`tests/bulk_crm_seam.rs`)
- **BSEAM-1 — bulk import creates REAL crm leads.** A lead-import batch drives crm's `create_lead` per item;
  two real leads land, each item records the created lead id. Zero normal Cargo edge.

## §5 round-trip (`scripts/bulk_crm_seam_roundtrip.sh`)
Regen (`--force`) leaves the seam files byte-identical; the oracle + seam re-run green.
