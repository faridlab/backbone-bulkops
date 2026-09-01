# Importer contract posture — base_import convergence onto bulk-ops

> Status: recorded contract. The import adapter itself is NOT built here — this module exposes the
> seam it will drive, and this document is the binding shape that adapter must take. Deviations from
> the Odoo `base_import` behavior are DECLARED deviations, recorded here on purpose.

## The contract, in one sentence

An importer is an **adapter producer driving `run_job` unchanged**: it parses a file, maps columns to
payload fields, and submits one `BulkJob` whose items each carry an idempotent `item_key`; the batch
engine, the target module's write path, and the audit ledger are exactly the ones every other bulk
operation already uses — no import-specific engine path exists or will be added.

## The four binding rules

1. **`run_job` unchanged.** The importer never bypasses, forks, or special-cases the run. Every item
   flows through the `BulkTargetPort` (the module's ONLY write seam — see `src/exports/services.rs`),
   so the target module's invariants are enforced on every imported row, and the (job, item_key)
   idempotency guard makes a re-import of the same file a no-op per item.

2. **Mapping persistence gates on `BulkJobCompleted`.** Column-to-field mapping memory is written
   only after the job's completion event — a partially-failed or crashed import must not poison the
   remembered mapping. A crashed run reaches that event through `reconcile_applying` (documented on
   the method in `src/application/service/bulk_write_service.rs`), which is why the reconciler
   publishes the same event when it empties the job's non-terminal set.

3. **Mapping memory is company-scoped under RLS.** Mapping rows live with the company whose import
   produced them (ADR-0008 / ADR-0010 posture — same fence as `bulk_jobs`/`bulk_job_items`).
   **Declared deviation vs Odoo**: `base_import` remembers mappings in a single global
   last-writer-wins slot (`ir.import_preview`-era options); two companies (or two operators) sharing
   one Odoo database overwrite each other's remembered mapping. Here a mapping is visible ONLY to
   the company that created it. The mapping store itself is owned host-side/family-side at import
   build time — bulk-ops deliberately holds no import-mapping schema in this increment; when it is
   built, this document is the contract it must satisfy.

4. **URL-fetch is fenced for this wave — file-upload only.** The Odoo import path accepts a remote
   URL to fetch a file from; that surface is SSRF-shaped and is NOT ported. The re-entry condition
   for any future URL fetch: an admin-gated allowlist (scheme + host regex, own-bucket endpoints
   only), hard size and timeout caps, redirects not followed across hosts, and a deny-by-default
   posture — enforced in code, never an assert. Until then the only intake is an operator-uploaded
   file.

## What the adapter owes the seam

- Parse + type-coerce (CSV first) and PREVIEW before submission: preview happens entirely
  adapter-side against the parsed rows; nothing lands in `bulk_job_items` until the operator confirms.
- One item per source row; `item_key` = a natural key the target can dedup on (the same key the
  `BulkTargetPort`'s `(company_id, item_key)` idempotency contract requires — a re-imported row
  cannot double).
- Errors surface through `failures()` / `retry_failed()` like any batch — the operator loop is the
  module's existing one.
