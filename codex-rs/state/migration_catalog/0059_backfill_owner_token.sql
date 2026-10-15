ALTER TABLE backfill_state ADD COLUMN owner_token TEXT;

-- Re-run the now insert-only backfill once so databases created by older
-- workers cannot retain a skipped rollout or an abandoned long lease.
UPDATE backfill_state
SET
    status = 'pending',
    last_watermark = NULL,
    last_success_at = NULL,
    owner_token = NULL,
    updated_at = CAST(strftime('%s', 'now') AS INTEGER)
WHERE id = 1;
