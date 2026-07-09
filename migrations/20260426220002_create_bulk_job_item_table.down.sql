-- Down: drop bulkops.bulk_job_items table
DROP TABLE IF EXISTS bulkops.bulk_job_items CASCADE;
DROP FUNCTION IF EXISTS bulkops.bulk_job_items_audit_timestamp() CASCADE;
