-- Every project's Project Manager: one Claude Code session per project, woken by what lands on
-- its project, by the results of its work, by a person, and by Elysia, which plans the project's
-- work and submits it as batches for the engine's worker pool. See docs/engine.md, Project
-- Managers.

-- The machine every Project Manager acts as. History names the project as well
-- (`project_manager:<project id>`), as a coding agent's names its session.
INSERT INTO users (id, name, role) VALUES
    ('00000000-0000-0000-0000-000000000004', 'Project manager', 'agent');

-- One row per project whose Project Manager has been woken: the session its turns resume, and
-- how long a usage limit holds it. Created the first time something wakes it.
CREATE TABLE project_managers (
    project_id UUID PRIMARY KEY REFERENCES projects (id) ON DELETE CASCADE,
    -- The Claude Code session the next turn resumes (`claude -p --resume`), kept indefinitely;
    -- Claude Code compacts it as it grows. NULL until the first turn reports one.
    runner_session_id TEXT,
    -- Set when every Claude credential reported a usage limit: no turn starts before this time.
    usage_paused_until TIMESTAMPTZ,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT project_managers_session_shape CHECK (char_length(runner_session_id) BETWEEN 1 AND 200)
);

SELECT diesel_manage_updated_at('project_managers');

CREATE TYPE project_manager_turn_status AS ENUM (
    'running',
    'succeeded',
    -- It ended without finishing; `error` says why.
    'failed'
);

-- One turn of a project's Project Manager. A turn is created as it starts, taking every wake
-- waiting for it, so a project has no queued turns: what waits is its wakes.
CREATE TABLE project_manager_turns (
    -- UUIDv7, so turns order as they started.
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES project_managers (project_id) ON DELETE CASCADE,
    status project_manager_turn_status NOT NULL DEFAULT 'running',
    -- One or two sentences on what the turn did and decided: its `Summary:` line.
    summary TEXT,
    -- What it answered, before its summary: shown under the message that woke it.
    reply TEXT,
    error TEXT,
    -- What it did, line by line: its messages and the tools it called, the newest 64 KiB kept.
    log TEXT NOT NULL DEFAULT '',
    -- The record it put on its project's timeline when it succeeded.
    timeline_record_id UUID REFERENCES timeline_records (id) ON DELETE SET NULL,
    -- The Project Manager's machine user: whatever woke it is on its wakes.
    created_by UUID NOT NULL REFERENCES users (id),
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,

    CONSTRAINT project_manager_turns_summary_length CHECK (char_length(summary) BETWEEN 1 AND 1000),
    CONSTRAINT project_manager_turns_reply_length CHECK (char_length(reply) BETWEEN 1 AND 20000),
    CONSTRAINT project_manager_turns_error_length CHECK (char_length(error) BETWEEN 1 AND 2000),
    CONSTRAINT project_manager_turns_log_length CHECK (octet_length(log) <= 65536),
    CONSTRAINT project_manager_turns_finished_at_set CHECK ((finished_at IS NULL) = (status = 'running')),
    CONSTRAINT project_manager_turns_error_set CHECK ((error IS NOT NULL) = (status = 'failed'))
);

-- A project's turns run one at a time: the database refuses a second running turn, whatever
-- the engine believes.
CREATE UNIQUE INDEX project_manager_turns_one_running
    ON project_manager_turns (project_id) WHERE status = 'running';

-- A project's turns, newest first.
CREATE INDEX project_manager_turns_by_project ON project_manager_turns (project_id, id DESC);

CREATE TYPE project_manager_wake_kind AS ENUM (
    -- A record was attributed to the project: a meeting's segment, a conversation's message,
    -- or a record a person put on it.
    'record',
    -- A batch it submitted finished.
    'batch',
    -- A person or Elysia wrote to it.
    'message'
);

-- What wakes a Project Manager, written in the same transaction as what caused it, so nothing
-- that should wake it is ever lost. A wake waits (`turn_id` NULL) until a turn takes it.
CREATE TABLE project_manager_wakes (
    -- UUIDv7, so wakes order as they arrived.
    id UUID PRIMARY KEY,
    -- The project, not its Project Manager's row, which its first turn creates: whatever wakes
    -- a Project Manager writes one row and nothing else.
    project_id UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    kind project_manager_wake_kind NOT NULL,
    timeline_record_id UUID REFERENCES timeline_records (id) ON DELETE CASCADE,
    work_batch_id UUID,
    -- A message's text.
    body TEXT,
    -- The turn that took it; NULL while it waits.
    turn_id UUID REFERENCES project_manager_turns (id) ON DELETE CASCADE,
    -- Who caused it: the person or Elysia who wrote, whoever attributed the record, or the
    -- system user for a batch's result.
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT project_manager_wakes_body_length CHECK (char_length(body) BETWEEN 1 AND 20000),
    -- Each kind carries exactly what it is about.
    CONSTRAINT project_manager_wakes_kind_fields CHECK (
        (timeline_record_id IS NOT NULL) = (kind = 'record')
        AND (work_batch_id IS NOT NULL) = (kind = 'batch')
        AND (body IS NOT NULL) = (kind = 'message')
    )
);

-- What waits, per project, in the order it arrived.
CREATE INDEX project_manager_wakes_waiting
    ON project_manager_wakes (project_id, created_at) WHERE turn_id IS NULL;

-- A turn's wakes, for its page.
CREATE INDEX project_manager_wakes_by_turn ON project_manager_wakes (turn_id);

-- A record waits once per project however often it is attributed meanwhile.
CREATE UNIQUE INDEX project_manager_wakes_record_waits_once
    ON project_manager_wakes (project_id, timeline_record_id) WHERE turn_id IS NULL;

CREATE TYPE work_batch_priority AS ENUM (
    -- Before every normal batch.
    'asap',
    'normal',
    -- At `scheduled_for`, before anything else once it is due.
    'scheduled'
);

CREATE TYPE work_batch_state AS ENUM (
    -- Waiting for a slot in the worker pool.
    'queued',
    -- A worker has it.
    'running',
    'succeeded',
    'failed',
    -- Withdrawn before a worker took it.
    'cancelled'
);

-- Work a Project Manager submits for the engine's worker pool: what it should accomplish, the
-- repositories it needs, when it should run, and what caused it. The scheduler takes queued
-- batches: due scheduled ones first, then ASAP, then normal, oldest first within each.
CREATE TABLE work_batches (
    -- UUIDv7, so batches order as they were submitted.
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    -- What the worker should accomplish: its brief, complete enough to work from alone.
    goal TEXT NOT NULL,
    -- The repositories it needs, as `[{ url, baseBranch? }]`, cloned before it starts.
    repositories JSONB NOT NULL DEFAULT '[]',
    priority work_batch_priority NOT NULL,
    -- When a scheduled batch is due; set exactly for scheduled batches.
    scheduled_for TIMESTAMPTZ,
    state work_batch_state NOT NULL DEFAULT 'queued',
    -- What its worker reported when it finished, or why it was cancelled.
    result TEXT,
    -- Provenance: the turn that submitted it, the timeline records that called for it, and the
    -- action items it works on.
    turn_id UUID REFERENCES project_manager_turns (id) ON DELETE SET NULL,
    -- Each a JSON array of ids, as the tool that submitted it was given them.
    source_record_ids JSONB NOT NULL DEFAULT '[]',
    action_item_ids JSONB NOT NULL DEFAULT '[]',
    -- The record it put on its project's timeline when it was submitted.
    timeline_record_id UUID REFERENCES timeline_records (id) ON DELETE SET NULL,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,

    CONSTRAINT work_batches_title_length CHECK (char_length(title) BETWEEN 1 AND 200),
    CONSTRAINT work_batches_goal_length CHECK (char_length(goal) BETWEEN 1 AND 20000),
    CONSTRAINT work_batches_result_length CHECK (char_length(result) BETWEEN 1 AND 20000),
    CONSTRAINT work_batches_repositories_array CHECK (
        jsonb_typeof(repositories) = 'array' AND jsonb_array_length(repositories) <= 16
    ),
    CONSTRAINT work_batches_source_record_ids_array CHECK (jsonb_typeof(source_record_ids) = 'array'),
    CONSTRAINT work_batches_action_item_ids_array CHECK (jsonb_typeof(action_item_ids) = 'array'),
    CONSTRAINT work_batches_scheduled_exactly_when_scheduled CHECK (
        (scheduled_for IS NOT NULL) = (priority = 'scheduled')
    ),
    -- A batch is cancelled only while it waits, so a cancelled one never started.
    CONSTRAINT work_batches_started_at_set CHECK (
        (started_at IS NULL) = (state IN ('queued', 'cancelled'))
    ),
    CONSTRAINT work_batches_finished_at_set CHECK (
        (finished_at IS NULL) = (state IN ('queued', 'running'))
    )
);

SELECT diesel_manage_updated_at('work_batches');

-- The scheduler's queue, in the order it hands out slots.
CREATE INDEX work_batches_queue
    ON work_batches (priority, scheduled_for, created_at) WHERE state = 'queued';

-- A project's batches, newest first.
CREATE INDEX work_batches_by_project ON work_batches (project_id, id DESC);

-- A batch's result wakes its Project Manager.
ALTER TABLE project_manager_wakes
    ADD CONSTRAINT project_manager_wakes_work_batch_id_fkey
    FOREIGN KEY (work_batch_id) REFERENCES work_batches (id) ON DELETE CASCADE;

-- A Project Manager acts in history, comments, changesets, and company doc proposals as
-- `project_manager:<project id>`, as a coding agent acts as `session:<number>`.
ALTER TABLE action_item_events DROP CONSTRAINT action_item_events_actor_shape;
ALTER TABLE action_item_events ADD CONSTRAINT action_item_events_actor_shape CHECK (
    actor ~ '^(user|user:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|elysia|session:[1-9][0-9]*|project_manager:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|watcher:[a-z][a-z0-9-]*|system)$'
);

ALTER TABLE action_item_comments DROP CONSTRAINT action_item_comments_author_shape;
ALTER TABLE action_item_comments ADD CONSTRAINT action_item_comments_author_shape CHECK (
    author ~ '^(user|user:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|elysia|session:[1-9][0-9]*|project_manager:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|watcher:[a-z][a-z0-9-]*|system)$'
);

ALTER TABLE changesets DROP CONSTRAINT changesets_proposer_shape;
ALTER TABLE changesets ADD CONSTRAINT changesets_proposer_shape CHECK (
    proposer ~ '^(elysia|session:[1-9][0-9]*|project_manager:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$'
);

ALTER TABLE company_doc_proposals DROP CONSTRAINT company_doc_proposals_agent_check;
ALTER TABLE company_doc_proposals ADD CONSTRAINT company_doc_proposals_agent_check CHECK (
    agent ~ '^(mcp:[^[:space:]]{1,255}|session:[0-9]{1,18}|project_manager:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$'
);
