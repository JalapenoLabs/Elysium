-- A Project Manager's batches start proposed, ahead of queued, and wait for a person to approve
-- them, edit and approve them, or reject them with a reason. The scheduler only ever hands a
-- slot to a queued batch, so nothing reaches a worker until a person says so. See
-- docs/engine.md, Work batches, and docs/autonomy.md, Proposed batches.

-- The type is rebuilt rather than given new values with ADD VALUE: this migration writes the new
-- values (the column's default, and the batches it holds for approval below), which Postgres
-- refuses for a value added in the same transaction. Everything comparing the column to a
-- literal is tied to the old type, so it is dropped first and added again after.
ALTER TABLE work_batches
    DROP CONSTRAINT work_batches_started_at_set,
    DROP CONSTRAINT work_batches_finished_at_set,
    DROP CONSTRAINT work_batches_assigned_at_set,
    DROP CONSTRAINT work_batches_claimed_while_in_a_slot,
    DROP CONSTRAINT work_batches_pinned_while_queued,
    DROP CONSTRAINT work_batches_waiting_whole;
DROP INDEX work_batches_queue;

ALTER TYPE work_batch_state RENAME TO work_batch_state_before_proposals;
CREATE TYPE work_batch_state AS ENUM (
    -- Submitted by its Project Manager; waits for a person to approve it before it may queue.
    'proposed',
    'queued',
    'paused',
    'assigned',
    'running',
    'succeeded',
    'failed',
    'cancelled',
    -- A person declined it, saying why; it never queued.
    'rejected'
);
ALTER TABLE work_batches ALTER COLUMN state DROP DEFAULT;
ALTER TABLE work_batches
    ALTER COLUMN state TYPE work_batch_state USING state::text::work_batch_state;
ALTER TABLE work_batches ALTER COLUMN state SET DEFAULT 'proposed';
DROP TYPE work_batch_state_before_proposals;

ALTER TABLE work_batches
    -- A worker starts a batch only once it holds a slot, and nothing waiting or declined ever
    -- started.
    ADD CONSTRAINT work_batches_started_at_set CHECK (
        (started_at IS NULL) = (state IN (
            'proposed', 'queued', 'paused', 'assigned', 'cancelled', 'rejected'
        ))
    ),
    ADD CONSTRAINT work_batches_finished_at_set CHECK (
        (finished_at IS NULL) = (state IN ('proposed', 'queued', 'paused', 'assigned', 'running'))
    ),
    -- What waits, or was declined before it queued, holds no slot; what holds one, or ran, got
    -- one. A cancelled batch may have held one.
    ADD CONSTRAINT work_batches_assigned_at_set CHECK (
        (state IN ('proposed', 'queued', 'paused', 'rejected') AND assigned_at IS NULL)
        OR (state IN ('assigned', 'running', 'succeeded', 'failed') AND assigned_at IS NOT NULL)
        OR state = 'cancelled'
    ),
    -- Exactly a batch holding a slot is claimed, under a lease, so a proposed batch can never
    -- be claimed whatever writes it.
    ADD CONSTRAINT work_batches_claimed_while_in_a_slot CHECK (
        (claim_token IS NOT NULL) = (state IN ('assigned', 'running'))
        AND (claim_token IS NULL) = (lease_expires_at IS NULL)
    ),
    ADD CONSTRAINT work_batches_pinned_while_queued CHECK (
        pinned_at IS NULL OR state = 'queued'
    ),
    ADD CONSTRAINT work_batches_waiting_whole CHECK (
        (waiting_on IS NULL) = (waiting_detail IS NULL)
        AND (waiting_on IS NULL) = (waiting_until IS NULL)
        AND (waiting_on IS NULL OR state = 'running')
    );

-- The scheduler's queue, in the order it hands out slots.
CREATE INDEX work_batches_queue
    ON work_batches (priority, scheduled_for, created_at) WHERE state = 'queued';

-- The batches waiting for a person's approval, per project, which the work queue lists and each
-- project's approval item counts.
CREATE INDEX work_batches_proposed ON work_batches (project_id, id) WHERE state = 'proposed';

-- A batch's history: a person's approval, the fields they changed first, and a rejection with
-- its reason, which is the feedback a rejection leaves (docs/self-improvement.md). A batch held
-- for approval when proposals began records where it was held from.
ALTER TABLE work_batch_events DROP CONSTRAINT work_batch_events_kind_known;
ALTER TABLE work_batch_events ADD CONSTRAINT work_batch_events_kind_known CHECK (kind IN (
    'submitted', 'held_for_approval', 'edited', 'approved', 'rejected', 'pinned', 'unpinned',
    'reprioritized', 'paused', 'resumed', 'cancelled', 'assigned', 'unassigned', 'started',
    'waiting', 'answered', 'finished'
));

-- Nothing waiting for a slot was approved by a person, so every batch still waiting goes back
-- for approval, its place at the front of the queue dropped; one in a slot, or running, goes on.
-- Its history says where it was held from, as the system user, before the move.
INSERT INTO work_batch_events (id, work_batch_id, kind, data, created_by)
SELECT uuidv7(), id, 'held_for_approval', jsonb_build_object('from', state::text),
    '00000000-0000-0000-0000-000000000001'
FROM work_batches
WHERE state IN ('queued', 'paused')
ORDER BY id;

UPDATE work_batches
SET state = 'proposed', pinned_at = NULL
WHERE state IN ('queued', 'paused');

-- The one inbox item per project that asks a person to approve its proposed batches, kept up to
-- date with how many wait and resolved once none are left. A project has at most one at a time;
-- when its person resolved or dismissed it by hand, the next proposal raises a new one here.
CREATE TABLE work_batch_approval_items (
    project_id UUID PRIMARY KEY REFERENCES projects (id) ON DELETE CASCADE,
    action_item_id UUID NOT NULL REFERENCES action_items (id) ON DELETE CASCADE,
    -- The Project Managers' machine user, whose submission raised it.
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

SELECT diesel_manage_updated_at('work_batch_approval_items');
