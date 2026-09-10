-- Hand-authored (user-owned). Not regenerated.
--
-- Strip every company-fence artifact from the bulkops tables (ADR-0029): the module is
-- tenant-agnostic; org scoping is installed by the COMPOSING service's tenancy decorator,
-- never by the module. Dropped here, per table: the company-leading indexes, the
-- <table>_company_isolation RLS policy, and the company_id column itself.
--
-- Tables: bulk_jobs and bulk_job_items.
--
-- Ordering guard (the decorator must run FIRST on any database with data): the module
-- never moves tenancy data. A table is safe to strip when EITHER
--   a) it carries org_unit_id with no NULLs — the decorator backfilled it from company_id —
--      or b) it is empty (a fresh database: the earlier chain files created it empty).
-- Otherwise the strip RAISEs, naming the decorator step, rather than dropping a column
-- that still holds the only tenancy key. The file is re-runnable (every drop is IF EXISTS
-- and the tracker has no checksums), so a failed run retries cleanly after the decorator
-- lands.
--
-- RLS enable/force flags are deliberately NOT touched: the decorator owns those now.
-- The (job_id, item_key) dedup unique is a DOMAIN invariant (the job pins the tenant) and
-- stays untouched, as do the audit triggers and the metadata indexes.

DO $$
DECLARE
    t text;
    has_org boolean;
    org_nulls bigint;
    total bigint;
    offenders text := '';
BEGIN
    FOREACH t IN ARRAY ARRAY['bulk_jobs', 'bulk_job_items']
    LOOP
        IF to_regclass(format('bulkops.%I', t)) IS NULL THEN
            CONTINUE; -- chain not fully applied on this database; nothing to strip
        END IF;

        SELECT EXISTS (
                   SELECT 1 FROM information_schema.columns
                   WHERE table_schema = 'bulkops' AND table_name = t AND column_name = 'org_unit_id'
               )
        INTO has_org;

        EXECUTE format('SELECT count(*) FROM bulkops.%I', t) INTO total;

        IF has_org THEN
            EXECUTE format(
                'SELECT count(*) FROM bulkops.%I WHERE org_unit_id IS NULL', t)
            INTO org_nulls;
        ELSE
            org_nulls := total; -- no org column: every row's only tenancy key is company_id
        END IF;

        IF has_org AND org_nulls = 0 THEN
            CONTINUE; -- decorator backfilled: safe
        END IF;
        IF total = 0 THEN
            CONTINUE; -- empty table (fresh database): safe
        END IF;
        offenders := offenders || format(' bulkops.%s (%s rows, %s rows not covered by org_unit_id);', t, total, org_nulls);
    END LOOP;

    IF offenders <> '' THEN
        RAISE EXCEPTION 'refusing to strip company_id — these tables are not yet covered by the tenancy decorator:%. Apply the composing service''s tenancy decorator (it backfills org_unit_id from company_id) and re-run; it is the only step that moves tenancy data.', offenders;
    END IF;
END $$;

-- ── bulk_jobs ─────────────────────────────────────────────────────────────────
DROP INDEX IF EXISTS bulkops.idx_bulk_jobs_company_id_status;
DROP POLICY IF EXISTS bulk_jobs_company_isolation ON bulkops.bulk_jobs;
ALTER TABLE bulkops.bulk_jobs DROP COLUMN IF EXISTS company_id;

-- ── bulk_job_items ────────────────────────────────────────────────────────────
DROP INDEX IF EXISTS bulkops.idx_bulk_job_items_company_id;
DROP POLICY IF EXISTS bulk_job_items_company_isolation ON bulkops.bulk_job_items;
ALTER TABLE bulkops.bulk_job_items DROP COLUMN IF EXISTS company_id;
