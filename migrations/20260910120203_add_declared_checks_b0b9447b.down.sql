-- Down: drop the declared rule constraints for bulkops module
ALTER TABLE IF EXISTS bulkops.bulk_job_items DROP CONSTRAINT IF EXISTS ck_bulk_job_items_error_detail_max_length__max_length;
ALTER TABLE IF EXISTS bulkops.bulk_job_items DROP CONSTRAINT IF EXISTS ck_bulk_job_items_applied_ref_type_max_length__max_length;
ALTER TABLE IF EXISTS bulkops.bulk_job_items DROP CONSTRAINT IF EXISTS ck_bulk_job_items_payload_max_length__max_length;
ALTER TABLE IF EXISTS bulkops.bulk_job_items DROP CONSTRAINT IF EXISTS ck_bulk_job_items_payload_required__required;
ALTER TABLE IF EXISTS bulkops.bulk_job_items DROP CONSTRAINT IF EXISTS ck_bulk_job_items_item_key_max_length__max_length;
ALTER TABLE IF EXISTS bulkops.bulk_job_items DROP CONSTRAINT IF EXISTS ck_bulk_job_items_item_key_required__required;
ALTER TABLE IF EXISTS bulkops.bulk_job_items DROP CONSTRAINT IF EXISTS ck_bulk_job_items_job_id_required__required;
DO $$
BEGIN
    IF to_regclass('bulkops.uq_bulk_job_items_item_key__unique') IS NOT NULL AND to_regclass('bulkops.idx_bulk_job_items_job_id_item_key') IS NULL THEN
        ALTER INDEX bulkops.uq_bulk_job_items_item_key__unique RENAME TO idx_bulk_job_items_job_id_item_key;
    END IF;
END $$;

ALTER TABLE IF EXISTS bulkops.bulk_jobs DROP CONSTRAINT IF EXISTS ck_bulk_jobs_target_module_max_length__max_length;
ALTER TABLE IF EXISTS bulkops.bulk_jobs DROP CONSTRAINT IF EXISTS ck_bulk_jobs_target_module_required__required;
ALTER TABLE IF EXISTS bulkops.bulk_jobs DROP CONSTRAINT IF EXISTS ck_bulk_jobs_operation_type_max_length__max_length;
ALTER TABLE IF EXISTS bulkops.bulk_jobs DROP CONSTRAINT IF EXISTS ck_bulk_jobs_operation_type_required__required;
