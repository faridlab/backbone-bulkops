-- Down: drop enum types for bulkops module
DROP TYPE IF EXISTS bulk_item_status CASCADE;
DROP TYPE IF EXISTS bulk_job_status CASCADE;
