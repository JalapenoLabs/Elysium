-- The kinds of agent Elysium runs (docs/engine.md, Models): every step and agent whose work is a
-- turn on the runner belongs to one, and a kind's model and effort apply to each of its agents
-- that has no setting of its own. The list is Elysium's own, written in code
-- (`agent_models::AgentKind`), so a native enum keeps a row from naming one no build knows.
CREATE TYPE agent_kind AS ENUM (
    'worker',
    'project_manager',
    'correction',
    'segmenting',
    'triage',
    'elysia',
    'summaries'
);

-- A kind's model and effort, chosen by an admin for the whole workspace (`project_id` NULL), or by
-- the people who edit a project for that project's own agents. A kind without a row follows what
-- the level beneath it resolves to, so a row exists only for a choice someone made.
CREATE TABLE kind_model_settings (
    id UUID PRIMARY KEY,
    -- The project whose agents it overrides; NULL for the workspace's setting of the kind.
    project_id UUID REFERENCES projects (id) ON DELETE CASCADE,
    kind agent_kind NOT NULL,
    -- What the official CLI's `--model` takes, as `model_settings.model` checks.
    model TEXT NOT NULL CHECK (model ~ '^[A-Za-z0-9][A-Za-z0-9._\[\]-]{0,99}$'),
    -- What the CLI's `--effort` takes, as `model_settings.effort` checks.
    effort TEXT NOT NULL CHECK (effort IN ('low', 'medium', 'high', 'xhigh', 'max')),
    -- Who first set it: an admin for the workspace's, the person editing the project otherwise.
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- One setting per kind for the workspace, and one per kind in each project.
    CONSTRAINT kind_model_settings_scope_unique UNIQUE NULLS NOT DISTINCT (project_id, kind),
    -- A project overrides only the kinds whose turns run for one project.
    CONSTRAINT kind_model_settings_project_kind_check
        CHECK (project_id IS NULL OR kind IN ('worker', 'project_manager', 'summaries'))
);

SELECT diesel_manage_updated_at('kind_model_settings');

-- What a kind's turns cost is read from the turns that record it: a session turn's `finished`
-- event carries its cost and tokens. Reading a window of them must not scan every event.
CREATE INDEX session_events_finished_at ON session_events (occurred_at)
    WHERE payload ->> 'kind' = 'finished';
