-- Enum values cannot be dropped from a PostgreSQL enum type in place; the removal path is the
-- standard rebuild (new type without the value, columns re-cast, old type dropped). Any row still
-- holding 'cancelled' must first be moved to a surviving status, which the caller decides — this
-- down migration therefore refuses rather than silently reclassifying audit rows.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM bulkops.bulk_job_items WHERE status = 'cancelled'::bulk_item_status)
       OR EXISTS (SELECT 1 FROM bulkops.bulk_jobs WHERE status = 'cancelled'::bulk_job_status) THEN
        RAISE EXCEPTION 'cannot drop cancelled states: rows still hold them (move them to a surviving status first)';
    END IF;
END
$$;

CREATE TYPE bulk_item_status_rebuilt AS ENUM ('pending', 'applying', 'applied', 'failed');
ALTER TABLE bulkops.bulk_job_items
    ALTER COLUMN status TYPE bulk_item_status_rebuilt
    USING (status::text::bulk_item_status_rebuilt);
DROP TYPE bulk_item_status;
ALTER TYPE bulk_item_status_rebuilt RENAME TO bulk_item_status;

CREATE TYPE bulk_job_status_rebuilt AS ENUM ('pending', 'running', 'completed', 'failed');
ALTER TABLE bulkops.bulk_jobs
    ALTER COLUMN status TYPE bulk_job_status_rebuilt
    USING (status::text::bulk_job_status_rebuilt);
DROP TYPE bulk_job_status;
ALTER TYPE bulk_job_status_rebuilt RENAME TO bulk_job_status;
