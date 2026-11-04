-- The engine (`elysium-engine`) does all background work, and the API serves clients alone.
-- What one process asks of the other, and what one shows of the other's work, is kept here,
-- so either can restart without the other losing it. See docs/engine.md, A service of its own.

-- Every turn a coding or Studio session was given. The API queues one; the engine takes it
-- when the session's previous turn has finished, runs it on the runner, and finishes it. The
-- turn's events are in `session_events`; this row is what the session is doing.
CREATE TYPE session_turn_status AS ENUM (
    -- Waiting for the engine.
    'queued',
    -- The engine took it: it waits for a slot on the runner, or runs (`started_at`).
    'running',
    -- Over, however it ended. Its outcome is its last event.
    'finished'
);

CREATE TABLE session_turns (
    -- The runner's turn id, which the turn's events carry too. Generated as UUIDv7.
    id UUID PRIMARY KEY,
    session_id BIGINT NOT NULL REFERENCES coding_sessions (id) ON DELETE CASCADE,
    prompt TEXT NOT NULL,
    -- Appended to the agent's system prompt for this turn, such as Studio's instructions.
    system_prompt_append TEXT,
    status session_turn_status NOT NULL DEFAULT 'queued',
    -- Someone asked for it to stop: the engine cancels it if it runs, and it never starts.
    stop_requested BOOLEAN NOT NULL DEFAULT FALSE,
    queued_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- When its CLI started, once it had a slot.
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    -- The person who gave the prompt.
    created_by UUID NOT NULL REFERENCES users (id),

    CONSTRAINT session_turns_finished_once_done CHECK (
        (status = 'finished') = (finished_at IS NOT NULL)
    ),
    CONSTRAINT session_turns_started_only_once_taken CHECK (
        started_at IS NULL OR status <> 'queued'
    )
);

-- What each session is doing, and the engine's queue: every turn not yet finished.
CREATE INDEX session_turns_unfinished ON session_turns (session_id, queued_at)
    WHERE status <> 'finished';

-- Per-turn access to Elysium's tool servers (`/api/agent-tools`). The engine grants one before
-- each turn and revokes it when the turn ends; the API checks it on every call. Only the
-- SHA-256 of the bearer token is kept, so the table never holds a usable credential.
CREATE TABLE agent_tool_grants (
    token_sha256 BYTEA PRIMARY KEY,
    -- Who the calls are made for: a session, with its project, item, and directory, or one of
    -- Elysia's turns. See `crate::tools::CallScope`.
    scope JSONB NOT NULL,
    -- The tool servers the turn may call, by name.
    servers TEXT[] NOT NULL,
    -- The coding agent, or Elysia.
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT agent_tool_grants_digest_length CHECK (octet_length(token_sha256) = 32)
);

-- Work a restart of the stack would cut short, such as an agent turn or a backup, as each
-- process holds it. Auto-update waits until no live row lists any. A row lives only while its
-- process renews it, so a process that died holds nothing.
CREATE TABLE process_activities (
    -- A process's own id, new at every start.
    process_id UUID PRIMARY KEY,
    -- What it is running, oldest first: `[{ "kind", "label", "startedAt" }]`.
    activities JSONB NOT NULL,
    -- Past this, the process is taken to have stopped, and the row counts for nothing.
    expires_at TIMESTAMPTZ NOT NULL,
    -- The system user: processes are Elysium's own.
    created_by UUID NOT NULL REFERENCES users (id)
);

-- What auto-update has learned, wherever it was learned: the engine checks Docker Hub, and the
-- API answers the updater and the admins. One row.
CREATE TABLE update_checks (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE,
    -- The newest release Docker Hub listed at the last check that answered.
    newest_release TEXT,
    last_checked_at TIMESTAMPTZ,
    -- Why the last check failed, until one succeeds.
    last_check_error TEXT,
    -- When the updater last asked what to do.
    updater_seen_at TIMESTAMPTZ,
    -- A release an admin asked for with Update now, even one that rolled back before, and who.
    requested_release TEXT,
    requested_by UUID REFERENCES users (id),
    updated_by UUID NOT NULL REFERENCES users (id),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT update_checks_one_row CHECK (singleton),
    CONSTRAINT update_checks_requested_by_someone CHECK (
        (requested_release IS NULL) = (requested_by IS NULL)
    )
);

INSERT INTO update_checks (updated_by) VALUES ('00000000-0000-0000-0000-000000000001');

-- What each browser's container is doing, as the engine's reconciler last found it.
CREATE TYPE browser_state AS ENUM (
    -- Its image is being pulled or its container started, or nothing has checked it yet.
    'starting',
    'running',
    -- The last attempt to start or check it failed, with the reason; the reconciler tries again.
    'failed'
);

ALTER TABLE browser_sessions
    ADD COLUMN state browser_state NOT NULL DEFAULT 'starting',
    ADD COLUMN state_error TEXT,
    ADD CONSTRAINT browser_sessions_error_when_failed CHECK (
        (state = 'failed') = (state_error IS NOT NULL)
    );

-- Set while a read someone asked for (Sync now), or a meeting's processing, is waiting or
-- under way, so two never run at once and the connector shows as syncing.
ALTER TABLE connectors ADD COLUMN syncing_since TIMESTAMPTZ;

-- When the engine took a backup someone asked for. A backup starts `running` the moment it is
-- asked for; one the engine never took when it stopped is still owed, not cut short.
ALTER TABLE database_backups ADD COLUMN claimed_at TIMESTAMPTZ;
