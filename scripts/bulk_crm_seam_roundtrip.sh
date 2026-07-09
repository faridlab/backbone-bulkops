#!/usr/bin/env bash
# §5 round-trip: the batch engine (idempotent per-item apply + partial-failure isolation) survives regen.
set -euo pipefail
cd "$(dirname "$0")/.."
export DATABASE_URL="${DATABASE_URL:-postgres://postgres:postgres@localhost:5433/backbone_bulkops}"
SEAM=(src/application/service/bulk_ports.rs src/application/service/bulk_events.rs src/application/service/bulk_write_service.rs)
before=$(shasum "${SEAM[@]}")
echo "== regenerating (--force) =="
metaphor schema schema generate --force >/dev/null
after=$(shasum "${SEAM[@]}")
if [[ "$before" != "$after" ]]; then echo "FAIL: seam files changed across regen"; diff <(echo "$before") <(echo "$after"); exit 1; fi
echo "OK: seam files byte-identical across regen"
echo "== re-running the oracle + seam =="
cargo test --test bulk_golden_cases --test integrity_probes --test bulk_crm_seam 2>&1 | grep -E "test result"
echo "OK: §5 round-trip holds"
