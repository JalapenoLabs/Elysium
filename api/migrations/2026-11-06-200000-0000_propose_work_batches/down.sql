DROP TABLE work_batch_approval_items;

-- A batch held for approval goes back to where it was held from. Every other proposal waits for
-- a slot again, as a batch submitted before proposals did, and a rejected one reads as withdrawn
-- with its reason, which its history keeps.
UPDATE work_batches
SET state = (held.data ->> 'from')::work_batch_state
FROM work_batch_events held
WHERE held.work_batch_id = work_batches.id
    AND held.kind = 'held_for_approval'
    AND work_batches.state = 'proposed';
UPDATE work_batches SET state = 'queued' WHERE state = 'proposed';
UPDATE work_batches SET state = 'cancelled' WHERE state = 'rejected';

UPDATE work_batch_events SET kind = 'cancelled' WHERE kind = 'rejected';
DELETE FROM work_batch_events WHERE kind IN ('held_for_approval', 'edited', 'approved');
ALTER TABLE work_batch_events DROP CONSTRAINT work_batch_events_kind_known;
ALTER TABLE work_batch_events ADD CONSTRAINT work_batch_events_kind_known CHECK (kind IN (
    'submitted', 'pinned', 'unpinned', 'reprioritized', 'paused', 'resumed', 'cancelled',
    'assigned', 'unassigned', 'started', 'waiting', 'answered', 'finished'
));

DROP INDEX work_batches_proposed;

-- Postgres cannot drop an enum's values, so the type is rebuilt without them, and everything
-- comparing the column to a literal is rebuilt with it, as the migrations before this one left it.
ALTER TABLE work_batches
    DROP CONSTRAINT work_batches_started_at_set,
    DROP CONSTRAINT work_batches_finished_at_set,
    DROP CONSTRAINT work_batches_assigned_at_set,
    DROP CONSTRAINT work_batches_claimed_while_in_a_slot,
    DROP CONSTRAINT work_batches_pinned_while_queued,
    DROP CONSTRAINT work_batches_waiting_whole;
DROP INDEX work_batches_queue;

ALTER TYPE work_batch_state RENAME TO work_batch_state_with_proposals;
CREATE TYPE work_batch_state AS ENUM (
    'queued', 'paused', 'assigned', 'running', 'succeeded', 'failed', 'cancelled'
);
ALTER TABLE work_batches ALTER COLUMN state DROP DEFAULT;
ALTER TABLE work_batches
    ALTER COLUMN state TYPE work_batch_state USING state::text::work_batch_state;
ALTER TABLE work_batches ALTER COLUMN state SET DEFAULT 'queued';
DROP TYPE work_batch_state_with_proposals;

ALTER TABLE work_batches
    ADD CONSTRAINT work_batches_started_at_set CHECK (
        (started_at IS NULL) = (state::text IN ('queued', 'paused', 'assigned', 'cancelled'))
    ),
    ADD CONSTRAINT work_batches_finished_at_set CHECK (
        (finished_at IS NULL) = (state::text IN ('queued', 'paused', 'assigned', 'running'))
    ),
    ADD CONSTRAINT work_batches_assigned_at_set CHECK (
        (state::text IN ('queued', 'paused') AND assigned_at IS NULL)
        OR (state::text IN ('assigned', 'running', 'succeeded', 'failed') AND assigned_at IS NOT NULL)
        OR state::text = 'cancelled'
    ),
    ADD CONSTRAINT work_batches_claimed_while_in_a_slot CHECK (
        (claim_token IS NOT NULL) = (state::text IN ('assigned', 'running'))
        AND (claim_token IS NULL) = (lease_expires_at IS NULL)
    ),
    ADD CONSTRAINT work_batches_pinned_while_queued CHECK (
        pinned_at IS NULL OR state::text = 'queued'
    ),
    ADD CONSTRAINT work_batches_waiting_whole CHECK (
        (waiting_on IS NULL) = (waiting_detail IS NULL)
        AND (waiting_on IS NULL) = (waiting_until IS NULL)
        AND (waiting_on IS NULL OR state = 'running')
    );

CREATE INDEX work_batches_queue
    ON work_batches (priority, scheduled_for, created_at) WHERE state = 'queued';
