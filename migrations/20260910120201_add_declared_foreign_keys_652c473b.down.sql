-- Down: drop the declared foreign keys for bulkops module
ALTER TABLE IF EXISTS bulkops.bulk_job_items DROP CONSTRAINT IF EXISTS fk_bulk_job_items_job_id;
