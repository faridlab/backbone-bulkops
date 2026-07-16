-- Down: remove the company RLS fence for bulkops module

-- Reverse the company RLS fence for bulkops.bulk_jobs
DROP POLICY IF EXISTS bulk_jobs_company_isolation ON bulkops.bulk_jobs;
ALTER TABLE bulkops.bulk_jobs NO FORCE ROW LEVEL SECURITY;
ALTER TABLE bulkops.bulk_jobs DISABLE ROW LEVEL SECURITY;

