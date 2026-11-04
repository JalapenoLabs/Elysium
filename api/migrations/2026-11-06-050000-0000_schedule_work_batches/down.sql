DROP TABLE worker_pool;

ALTER TABLE workspace_settings
    DROP CONSTRAINT workspace_settings_worker_pool_size_positive,
    DROP COLUMN worker_pool_size;

DROP TABLE work_batch_events;

DROP INDEX work_batches_in_slots;

ALTER TABLE work_batches
    DROP CONSTRAINT work_batches_pinned_while_queued,
    DROP CONSTRAINT work_batches_claimed_while_in_a_slot,
    DROP CONSTRAINT work_batches_assigned_at_set,
    DROP CONSTRAINT work_batches_started_at_set,
    DROP CONSTRAINT work_batches_finished_at_set;

-- What waited, paused or in a slot no worker took, waits again.
UPDATE work_batches SET state = 'queued' WHERE state::text IN ('paused', 'assigned');

ALTER TABLE work_batches
    DROP COLUMN pinned_at,
    DROP COLUMN assigned_at,
    DROP COLUMN claim_token,
    DROP COLUMN lease_expires_at;

-- Postgres cannot drop an enum's values, so the type is rebuilt without them; everything
-- comparing the column to a literal is tied to the old type and is rebuilt with it.
DROP INDEX work_batches_queue;
ALTER TYPE work_batch_state RENAME TO work_batch_state_with_slots;
CREATE TYPE work_batch_state AS ENUM ('queued', 'running', 'succeeded', 'failed', 'cancelled');
ALTER TABLE work_batches ALTER COLUMN state DROP DEFAULT;
ALTER TABLE work_batches
    ALTER COLUMN state TYPE work_batch_state USING state::text::work_batch_state;
ALTER TABLE work_batches ALTER COLUMN state SET DEFAULT 'queued';
DROP TYPE work_batch_state_with_slots;

ALTER TABLE work_batches
    ADD CONSTRAINT work_batches_started_at_set CHECK (
        (started_at IS NULL) = (state IN ('queued', 'cancelled'))
    ),
    ADD CONSTRAINT work_batches_finished_at_set CHECK (
        (finished_at IS NULL) = (state IN ('queued', 'running'))
    );

CREATE INDEX work_batches_queue
    ON work_batches (priority, scheduled_for, created_at) WHERE state = 'queued';
