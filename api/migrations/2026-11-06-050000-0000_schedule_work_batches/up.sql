-- The engine's worker pool hands out slots to work batches: due scheduled batches first, then
-- ASAP, then normal, oldest first within each, round robin across projects. People and Elysia
-- may run a batch next, change its priority, pause it, or cancel it while it waits, each written
-- to the batch's history. See docs/engine.md, Workers and the pool.

-- A batch may wait held back by a person or Elysia, and may hold a slot before a worker starts
-- on it. New values are compared as text below: a value added in this transaction cannot be
-- used as an enum literal before it commits.
ALTER TYPE work_batch_state ADD VALUE 'paused' AFTER 'queued';
ALTER TYPE work_batch_state ADD VALUE 'assigned' AFTER 'paused';

ALTER TABLE work_batches
    -- Set while a person or Elysia asked for it to run next: such batches go before every
    -- other, the earliest asked first.
    ADD COLUMN pinned_at TIMESTAMPTZ,
    -- When the pool gave it a slot, kept once a worker takes it.
    ADD COLUMN assigned_at TIMESTAMPTZ,
    -- The engine holding its slot, and until when unless renewed: an engine that stops leaves
    -- its batches for the next to take once their leases lapse.
    ADD COLUMN claim_token UUID,
    ADD COLUMN lease_expires_at TIMESTAMPTZ;

-- No worker has run a batch yet, but a row that says otherwise keeps a consistent claim.
UPDATE work_batches
SET assigned_at = started_at,
    claim_token = CASE WHEN state = 'running' THEN gen_random_uuid() END,
    lease_expires_at = CASE WHEN state = 'running' THEN now() END
WHERE started_at IS NOT NULL;

ALTER TABLE work_batches DROP CONSTRAINT work_batches_started_at_set;
ALTER TABLE work_batches DROP CONSTRAINT work_batches_finished_at_set;

ALTER TABLE work_batches
    -- A worker starts a batch only once it holds a slot, and a waiting one is never started.
    ADD CONSTRAINT work_batches_started_at_set CHECK (
        (started_at IS NULL) = (state::text IN ('queued', 'paused', 'assigned', 'cancelled'))
    ),
    ADD CONSTRAINT work_batches_finished_at_set CHECK (
        (finished_at IS NULL) = (state::text IN ('queued', 'paused', 'assigned', 'running'))
    ),
    -- What waits holds no slot; what holds one, or ran, got one. A cancelled batch may have
    -- held one.
    ADD CONSTRAINT work_batches_assigned_at_set CHECK (
        (state::text IN ('queued', 'paused') AND assigned_at IS NULL)
        OR (state::text IN ('assigned', 'running', 'succeeded', 'failed') AND assigned_at IS NOT NULL)
        OR state::text = 'cancelled'
    ),
    -- Exactly a batch holding a slot is claimed, under a lease.
    ADD CONSTRAINT work_batches_claimed_while_in_a_slot CHECK (
        (claim_token IS NOT NULL) = (state::text IN ('assigned', 'running'))
        AND (claim_token IS NULL) = (lease_expires_at IS NULL)
    ),
    ADD CONSTRAINT work_batches_pinned_while_queued CHECK (
        pinned_at IS NULL OR state::text = 'queued'
    );

-- The slots in use, per project, for the scheduler's round robin.
CREATE INDEX work_batches_in_slots ON work_batches (project_id) WHERE claim_token IS NOT NULL;

-- A batch's history: its submission, every change a person, Elysia, or its Project Manager made
-- while it waited, and each move the pool and its worker made.
CREATE TABLE work_batch_events (
    -- UUIDv7, so entries order as written.
    id UUID PRIMARY KEY,
    work_batch_id UUID NOT NULL REFERENCES work_batches (id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    -- Shaped by `kind`; see docs/engine.md, Work batches.
    data JSONB NOT NULL DEFAULT '{}',
    -- Who did it: a person, Elysia, the Project Managers' machine user, or the system user for
    -- the pool.
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT work_batch_events_kind_known CHECK (kind IN (
        'submitted', 'pinned', 'unpinned', 'reprioritized', 'paused', 'resumed', 'cancelled',
        'assigned', 'unassigned', 'started', 'finished'
    )),
    CONSTRAINT work_batch_events_data_object CHECK (jsonb_typeof(data) = 'object')
);

CREATE INDEX work_batch_events_by_batch ON work_batch_events (work_batch_id, id);

-- Every batch already submitted begins its history there.
INSERT INTO work_batch_events (id, work_batch_id, kind, created_by, created_at)
SELECT uuidv7(), id, 'submitted', created_by, created_at FROM work_batches;

-- How many workers run at once, chosen by an admin; NULL follows the default, one per CPU core
-- the engine sees minus two, at least one.
ALTER TABLE workspace_settings
    ADD COLUMN worker_pool_size INTEGER,
    ADD CONSTRAINT workspace_settings_worker_pool_size_positive CHECK (worker_pool_size >= 1);

-- What the engine's pool knows that the queue page shows: the CPU cores it sees, which the
-- default size follows, and how long every Claude account's usage holds slots back. One row,
-- written by the engine as it looks.
CREATE TABLE worker_pool (
    singleton BOOLEAN PRIMARY KEY DEFAULT true,
    cpu_count INTEGER NOT NULL,
    -- Set while every Claude credential that could serve is at its usage limit: no slot is
    -- handed out before this time.
    usage_held_until TIMESTAMPTZ,
    created_by UUID NOT NULL REFERENCES users (id),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT worker_pool_singleton CHECK (singleton),
    CONSTRAINT worker_pool_cpu_count_positive CHECK (cpu_count >= 1)
);

SELECT diesel_manage_updated_at('worker_pool');
