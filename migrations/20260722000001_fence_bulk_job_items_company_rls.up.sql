-- ADR-0010 Decision A: fence bulkops.bulk_job_items by company_id.
-- Parent (bulk_jobs) is already fenced; the child was not.
ALTER TABLE bulkops.bulk_job_items ADD COLUMN IF NOT EXISTS company_id UUID;
UPDATE bulkops.bulk_job_items AS i
   SET company_id = j.company_id
  FROM bulkops.bulk_jobs AS j
 WHERE i.job_id = j.id AND i.company_id IS NULL;
ALTER TABLE bulkops.bulk_job_items ALTER COLUMN company_id SET NOT NULL;
CREATE INDEX IF NOT EXISTS idx_bulk_job_items_company_id ON bulkops.bulk_job_items (company_id);
ALTER TABLE bulkops.bulk_job_items ENABLE ROW LEVEL SECURITY;
ALTER TABLE bulkops.bulk_job_items FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS bulk_job_items_company_isolation ON bulkops.bulk_job_items;
CREATE POLICY bulk_job_items_company_isolation ON bulkops.bulk_job_items
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
