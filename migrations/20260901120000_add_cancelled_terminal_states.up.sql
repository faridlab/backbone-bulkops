-- Terminal `cancelled` states for crash recovery: a runner that dies between the target's commit
-- and the engine's outcome mark leaves an item (and its job) stuck in `applying` forever. The
-- reconciler re-checks such items against the target by item_key; an item the target can neither
-- confirm nor safely re-apply exits to `cancelled` instead of haunting the job as `applying`.
-- A job whose reconcile left a cancelled item (and no failed one) is itself `cancelled`.

ALTER TYPE bulk_job_status ADD VALUE IF NOT EXISTS 'cancelled';
ALTER TYPE bulk_item_status ADD VALUE IF NOT EXISTS 'cancelled';
