DROP POLICY IF EXISTS bulk_job_items_company_isolation ON bulkops.bulk_job_items;
ALTER TABLE bulkops.bulk_job_items NO FORCE ROW LEVEL SECURITY;
ALTER TABLE bulkops.bulk_job_items DISABLE ROW LEVEL SECURITY;
DROP INDEX IF EXISTS bulkops.idx_bulk_job_items_company_id;
ALTER TABLE bulkops.bulk_job_items DROP COLUMN IF EXISTS company_id;
