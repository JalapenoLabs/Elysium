ALTER TABLE work_batches
    DROP COLUMN source_message_ids,
    DROP COLUMN parent_batch_ids,
    DROP COLUMN generation,
    DROP COLUMN origin;

DROP TYPE work_batch_origin;
