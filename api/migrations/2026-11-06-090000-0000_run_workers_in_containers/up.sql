-- Each worker of the pool runs its batch in a container of its own: the repositories its project
-- declares are cloned and set up, then Claude Code works the batch to completion, keeping its
-- container while it waits on a review, CI, or an answer. See docs/engine.md, Workers.

-- The repositories a project's workers clone, in order, each with what readies it once cloned.
CREATE TABLE project_repositories (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    -- Where it is listed, from 0; a batch's repositories are cloned in the batch's own order.
    position INTEGER NOT NULL,
    url TEXT NOT NULL,
    -- The branch work starts from; the remote's default when unset.
    base_branch TEXT,
    -- Bash, run as the agent in the repository's directory once it is cloned.
    setup_script TEXT,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT project_repositories_position_non_negative CHECK (position >= 0),
    CONSTRAINT project_repositories_url_length CHECK (char_length(url) BETWEEN 1 AND 2048),
    CONSTRAINT project_repositories_base_branch_length CHECK (
        char_length(base_branch) BETWEEN 1 AND 255
    ),
    CONSTRAINT project_repositories_setup_script_length CHECK (
        char_length(setup_script) BETWEEN 1 AND 20000
    ),
    CONSTRAINT project_repositories_position_unique UNIQUE (project_id, position)
        DEFERRABLE INITIALLY DEFERRED
);

ALTER TABLE projects
    -- Bash, run as root in every worker's container before its repositories are cloned: the
    -- tools the project's work needs beyond the workspace image.
    ADD COLUMN tools_setup_script TEXT,
    ADD CONSTRAINT projects_tools_setup_script_length CHECK (
        char_length(tools_setup_script) BETWEEN 1 AND 20000
    );

-- What a running batch's worker waits on, its container kept meanwhile.
CREATE TYPE work_batch_wait AS ENUM ('review', 'ci', 'question');

ALTER TABLE work_batches
    -- The Claude Code session its worker's turns resume.
    ADD COLUMN worker_session_id TEXT,
    -- What its worker said it waits on, in its words, and when it looks again.
    ADD COLUMN waiting_on work_batch_wait,
    ADD COLUMN waiting_detail TEXT,
    ADD COLUMN waiting_until TIMESTAMPTZ,
    -- How its worker said its last turn ended, through its tools, for the engine to act on once
    -- the turn is over: `{ kind: "finished", succeeded, result }` or `{ kind: "waiting", on,
    -- detail, until }`.
    ADD COLUMN worker_report JSONB,
    -- What its worker said and which tools it called, one line each, scrubbed of its secrets,
    -- the newest 64 KiB kept.
    ADD COLUMN worker_log TEXT NOT NULL DEFAULT '',

    ADD CONSTRAINT work_batches_waiting_detail_length CHECK (
        char_length(waiting_detail) BETWEEN 1 AND 4000
    ),
    -- A wait is its kind, what it waits for, and when to look again, together, and only while
    -- a worker has the batch.
    ADD CONSTRAINT work_batches_waiting_whole CHECK (
        (waiting_on IS NULL) = (waiting_detail IS NULL)
        AND (waiting_on IS NULL) = (waiting_until IS NULL)
        AND (waiting_on IS NULL OR state = 'running')
    ),
    ADD CONSTRAINT work_batches_worker_report_object CHECK (
        jsonb_typeof(worker_report) = 'object'
    ),
    ADD CONSTRAINT work_batches_worker_log_size CHECK (octet_length(worker_log) <= 65536);

-- A worker's history: what it waits on, and the answers its Project Manager gave.
ALTER TABLE work_batch_events DROP CONSTRAINT work_batch_events_kind_known;
ALTER TABLE work_batch_events ADD CONSTRAINT work_batch_events_kind_known CHECK (kind IN (
    'submitted', 'pinned', 'unpinned', 'reprioritized', 'paused', 'resumed', 'cancelled',
    'assigned', 'unassigned', 'started', 'waiting', 'answered', 'finished'
));
