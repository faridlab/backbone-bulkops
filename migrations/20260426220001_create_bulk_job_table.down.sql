-- Down: drop bulkops.bulk_jobs table
DROP TABLE IF EXISTS bulkops.bulk_jobs CASCADE;
DROP FUNCTION IF EXISTS bulkops.bulk_jobs_audit_timestamp() CASCADE;
