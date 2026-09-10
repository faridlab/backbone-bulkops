-- Hand-authored (user-owned). Not regenerated.
--
-- Best-effort restore sketch for the tenancy strip (ADR-0029). This is a breaking module
-- release against dev-stage databases: the down re-adds the company_id column as nullable
-- with its plain indexes and the company isolation policy shapes, but restores NO data —
-- rows written after the strip (or after the decorator re-keyed them) carry org_unit_id
-- only. The composing service's tenancy decorator remains the live fence; treat this
-- down as a schema-shape sketch for archaeology, not a usable rollback.

ALTER TABLE bulkops.bulk_jobs      ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE bulkops.bulk_job_items ADD COLUMN IF NOT EXISTS company_id uuid;

CREATE INDEX IF NOT EXISTS idx_bulk_jobs_company_id_status ON bulkops.bulk_jobs (company_id, status);
CREATE INDEX IF NOT EXISTS idx_bulk_job_items_company_id   ON bulkops.bulk_job_items (company_id);

CREATE POLICY bulk_jobs_company_isolation ON bulkops.bulk_jobs
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
CREATE POLICY bulk_job_items_company_isolation ON bulkops.bulk_job_items
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
