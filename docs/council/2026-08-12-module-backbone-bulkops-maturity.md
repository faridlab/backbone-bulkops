<!--
date: 2026-08-12 | repo_type: module | unit: backbone-bulkops | focus: maturity
roster: chair, skeptic, steelman, yagni-business (standing) · ddd-bounded-context, contract-seat (module context) · domain-expert (invited — bulk-ops encodes real domain rules)
scope: single unit — the backbone-bulkops crate + its outward interactions
note: skeptic/steelman/chair ran as isolated subagents; their load-bearing claim (superuser test role bypasses RLS) was independently re-verified against the code by the orchestrator before synthesis. Regen safety was separately proven via an empirical `metaphor schema generate:rust --force` copy-test (0 deleted, 0 changed).
-->

# Council — module:backbone-bulkops — focus: maturity

## Best call

**Run the Skeptic's RLS non-superuser probe — one ~40-line integration test that connects a `NOBYPASSRLS` non-superuser role scoped to company B and invokes `run_job(job_A, company_B, &target, &sink)`, asserting (a) `Err(NotFound("job"))`, (b) `target.apply_count() == 0`, (c) company-A items unchanged.** Role via `CREATE ROLE bulkops_tenant NOBYPASSRLS; GRANT …; SET ROLE bulkops_tenant;` on a second pool.

This is the single missing fact that the entire maturity verdict hangs on. Every seat's positive case (Steelman's "well-engineered and well-tested," ADR-0008's ID-only scope contract, ADR-0010's fail-closed RLS) treats tenant isolation as a *verified* property. It is not: `tests/common/mod.rs:13-16` connects as the `postgres` superuser, every per-item SQL is ID-only (`WHERE id=$1`, `WHERE job_id=$1`), and no test exercises cross-tenant NotFound, fail-closed missing scope, or a non-superuser role. The all-green suite is observationally indistinguishable from "RLS works" and "RLS is silently inert."

- **Residual negative value:** Cost of the probe is ~1-2 engineer-hours and ~40 LOC. If green, the remaining maturity gaps (outward contract, bounded-context, crash-reconciler) still cost ~3-4 engineer-days to close, but the security story moves from asserted to verified and the module becomes honestly gradeable. If red, the probe has revealed a tenant-isolation hole whose blast radius is the *entire multi-tenant data plane of every downstream service that consumes this module* — a potentially catastrophic cross-tenant exposure; knowing is strictly cheaper than not knowing. The residual negative of *not* running it is that the first consumer either trusts an unverified comment (exposure risk) or re-derives this test themselves (duplicated effort, usually skipped).
- **Reversibility:** 100%. It is a test plus a throwaway role grant. Zero coupling added, zero production risk surface, trivially deletable if flaky.
- **What would flip it to a different Best call:** if ADR-0010 explicitly scoped RLS as "consumer's responsibility — this module is single-tenant," the fence claim would leave this module's maturity surface and the Best call would shift to the contract move (promote `BulkTargetPort` into `exports/`). It does not — ADR-0010 explicitly claims fail-closed RLS as this module's property, so the claim is in-scope and unverified.

## Disagreement map

**1. What bar does a *consumed* module library crate need to clear to be "done"?**
Crux: does a module discharge its central claims by ADR + DDL + golden-path tests, or by an executed assertion of the negative path?
- *Steelman side:* the engine is genuinely well-built (CAS reservation, idempotency, per-item isolation, DB-side rollups), regen is byte-identical, BGC-1..5 + BIP-4 race probe + BSEAM-1 real-CRM seam are rare quality for 0.3.0.
- *Skeptic / YAGNI side:* the central claim is unfalsifiable under the current test role, the 4 bulk verbs that ARE the point have no HTTP surface, and ~24 generic CRUD endpoints the module itself warns against are the mounted surface.
- *Chair picks Skeptic/YAGNI:* a consumed module's claims must be *verified*, not *asserted* — the consumer cannot re-litigate them.

**2. Is the RLS fence reliable enough to build a multi-tenant product on?**
Crux: ADR-as-intent vs test-as-proof.
- *Steelman side:* ADR-0008 puts `company_id` on `run_job`, every repo method rides `company_scope::*_scoped`, ADR-0010 backfills the child table and ENABLE & FORCE RLS with a fail-closed policy.
- *Skeptic side:* the migration itself says the app must connect as a non-superuser; the tests connect as the superuser; no executed assertion exists.
- *Chair picks Skeptic:* an ADR is a promise a reviewer cannot keep; a test under the deployed role is the only artifact that survives an inattentive future edit.

**3. Where is the outward contract boundary?**
Crux: is `exports/` the documented contract surface, or a convenience re-export?
- *Contract-seat:* `BulkTargetPort` + `BulkOp`/`BulkAck`/`BulkRejected` + a `BulkWriteService` handle belong in `exports/` (CLAUDE.md says "ONLY types other modules should use"); the composing consumer and the seam test both reach past the boundary today.
- *Current layout:* the port lives at `application::service::bulk_ports` (internal), `exports/services.rs` is empty, and `BulkWriteService::new(pool)` is called via an internal ctor.
- *Chair picks Contract-seat:* the project CLAUDE.md is unambiguous that `exports/` is the boundary; an internal path that consumers must reach is a stability hazard.

**4. Does documenting a gap discharge the completeness obligation?**
Crux: honesty vs completeness on the real failure mode.
- *Domain-Expert:* a runner *will* crash between `target.apply()` commit and `mark_applied`; `fetch_pending` skips `applying`, `retry_failed` touches only `failed` — the item is stranded with no reconciler. This is the most common batch failure mode and the module's reason-to-exist is reliable batch execution.
- *Steelman:* the gap is named at `bulk_ports.rs:38-43`.
- *Chair picks Domain-Expert on substance, but defers sequencing:* documenting a known unrecoverable stuck-state is honesty, not completeness — but it is additive (fix-forward), not a false claim, so it ranks below the unverified security claim on the maturity bar.

## Recommendations (ranked by leverage)

| # | Move | Leverage | Residual negative | Reversibility | Evidence to flip |
|---|------|----------|-------------------|---------------|------------------|
| 1 | Run the RLS non-superuser probe (~40 LOC, NOBYPASSRLS role, cross-tenant `run_job` asserting NotFound + zero apply) | Unblocks the maturity verdict on the highest-blast-radius claim; converts an assertion into proof (or reveals a hole) | ~1-2 hrs; if red, a real cross-tenant exposure that blocks ship | 100% — test + role grant only | A finding that ADR-0010 explicitly outsources RLS to the consumer (it does not) |
| 2 | Promote `BulkTargetPort` + `BulkOp`/`BulkAck`/`BulkRejected` + a `BulkWriteService` handle into `exports/` (the CUSTOM SERVICES block is already empty and waiting) | Stabilizes the outward contract so consumers stop reaching past the boundary; future port moves no longer break them | ~half-day; consumers must migrate one import path | High — re-export change | A decision that `exports/` is convenience-only, not contract (contradicts CLAUDE.md) |
| 3 | Delete the orphan Example scaffold (7 dead files) + drop `pub mod example_*` from `dto/mod.rs:8` and `lib.rs:39` | Removes a compiled second bounded context from the public surface; kills the "scaffold not yet specialized" read | ~1 hr; none functional — orchestrator proved the generator neither emits nor prunes these | 100% — regen-safe, git-reversible | A consumer actually importing `ExampleSagaFlowStep` (none found) |
| 4 | Add `reconcile_applying(job_id, older_than)` (re-check via port by `item_key`, confirm-applied or requeue) + a `cancelled` terminal state (the `applying` enum variant is already reserved) | Closes the most-common failure mode; discharges the documented gap at `bulk_ports.rs:38-43` | ~1-2 days; deferring strands items on the first crash in production | Additive feature, high | Proof that batches are always small/operator-watched and crash-mid-apply is implausible |
| 5 | Wire `BulkWriteService` into `BulkopsModule` (lib.rs:57-62 holds only generic CRUD today) + expose the 4 bulk verbs over HTTP; drop or feature-gate the unguarded generic CRUD the module warns against | Finishes the "module is mountable by a backend-service" story; removes the inverted surface (throwaway CRUD mounted, valuable verbs not) | ~2-3 days; a consumer could compose the service directly instead | Medium — touches module builder + routes | A decision that this crate stays a pure library with no first-class HTTP surface |
| 6 | Replace `PermitAllPolicy` + commented-out specifications with real domain rules OR delete the stub ceremony; fill feature-flag bodies; fix Cargo.toml description ("skeleton" → real) | Removes ~zero-cost ceremony that reads as "scaffold not yet specialized"; honesty in the crate manifest | ~1 day; mostly cosmetic but signal-improving | High | A rule that domain policy genuinely is permit-all at this layer |
| 7 | Migrate the live-DB integration tests to a non-superuser test role (beyond just the new probe in #1) so the whole suite runs under the deployed privilege level | Makes the fence's firing continuously verifiable, not just at the one probe point | ~1 day; test harness plumbing | High | The probe in #1 comes back green AND a review shows ID-only SQL is fully covered by RLS predicates |

## Maturity scorecard

| Axis | Score | Why |
|------|-------|-----|
| Engine correctness | 4 | CAS `pending→applying` reservation before external `port.apply()`, per-item isolation, `(job,item_key)` idempotency via `ON CONFLICT DO NOTHING`, DB-side rollups with `count(*) FILTER`, operator-driven `retry_failed` — docked one for strictly sequential apply (throughput ceiling) and no crash reconciler. |
| Tenant-isolation / security | 2 | The fence is asserted by ADR-0008/0010 and `company_scope::*_scoped` repo methods but is *unfalsifiable* under the current superuser test role; every per-item SQL is ID-only with zero `company_id` predicates and zero negative-path assertions — the maturity-killer until a non-superuser probe runs. |
| Outward contract | 2 | DTO contract itself is clean (typed IDs, summaries, no entity leak), but the central `BulkTargetPort` promise and the `BulkWriteService` value live at internal paths while `exports/services.rs` ships empty — consumers and the seam test reach past the boundary. |
| Bounded-context cleanliness | 3 | Core language (`BulkJob→BulkJobItem→BulkOp→BulkAck/BulkRejected`, documented state machine) is clean and consistent, but a vestigial `Example` second context is compiled into the public surface (`lib.rs:39`, `application/dto/mod.rs:8`) — 7 dead files on disk. |
| Domain completeness | 3 | Core batch rules are honored, but the stranded `applying` item after a crash has no reconcile path and there is no `cancelled`/`expired` terminal state, so an abandoned half-applied batch lives as `running` forever — real divergences, partially mitigated by being documented. |
| Regen / codegen contract | 5 | Orchestrator proved byte-identical regen on a full copy of the live tree (`metaphor schema generate:rust --force`: 0 deleted, 0 changed), CUSTOM blocks preserved, `user_owned` globs correct, migrations preserved — this is a genuine, rare strength. |
| Test maturity | 2 | Integration coverage is *thoughtful* (BGC-1..5 golden cases, BIP-4 real `tokio::join!` race proving CAS prevents double-apply, BSEAM-1 driving the real backbone_crm write path), but it is 100% live-DB superuser integration with zero unit tests and zero negative-path security assertions — the suite cannot falsify its central claim. |

**Aggregate read:** the regen contract and the engine internals are mature; the *consumed-module* surface (verified security claim, explicit contract boundary, clean public context) is not. The module is a strong 0.3.0 engine wearing a half-finished module jacket. The probe in the Best call is the cheapest possible move that converts the biggest assertion into evidence.

## Parking lot

- Auto retry/backoff policy for failed items (currently operator-driven `retry_failed` only).
- Parallel/concurrent item application to lift the sequential throughput ceiling.
- Observability surface: structured tracing/metrics for batch progress and item-level outcomes.
- gRPC / protobuf / graphql surfaces — honestly disabled today; defer until a consumer actually asks.
- Feature-flag bodies in `Cargo.toml` (currently empty stubs) — either wire or remove.
- Full migration of the integration suite to a non-superuser test role (beyond the one probe in Recommendation #1).
- A `cancelled`/`expired` terminal state for abandoned half-applied batches (folded into Recommendation #4 but separable).
