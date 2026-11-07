-- Workers and Project Managers run only with `~/.claude` from the configuration repository. The
-- repository is read with a GitHub token of its own, chosen beside it, since it may be private to
-- an account the workspace's default token cannot reach. See docs/coding.md, `~/.claude`.

-- NULL reads the repository without a token: a public one, or one off github.com. Deleting the
-- token leaves the repository unreadable until another is chosen, which holds workers back.
ALTER TABLE workspace_settings
    ADD COLUMN agent_configuration_github_credential_id UUID
        REFERENCES github_credentials (id) ON DELETE SET NULL;

-- A github.com repository was read with the workspace's default token; it keeps that token as
-- its own choice, so nothing an admin set up stops working.
UPDATE workspace_settings
SET agent_configuration_github_credential_id = (
    SELECT id FROM github_credentials WHERE is_default
)
WHERE agent_configuration_repository LIKE 'https://github.com/%';

-- How the last fetch of the configuration repository went, which decides whether workers and
-- Project Managers may start. One row, written by the engine as it fetches.
CREATE TABLE agent_configuration_status (
    singleton BOOLEAN PRIMARY KEY DEFAULT true,
    -- The repository and token the last attempt read. A status about another repository or
    -- token than the settings name says nothing about the current one.
    repository TEXT,
    github_credential_id UUID,
    -- The commit the last good fetch checked out, its subject, and when.
    commit_sha TEXT,
    commit_subject TEXT,
    fetched_at TIMESTAMPTZ,
    -- What the checkout held that the workspace cannot run, and so left out: `[{ kind, name,
    -- reason, detail }]`. See crate::runner::configuration::adapt.
    skipped JSONB NOT NULL DEFAULT '[]',
    -- Set while the last attempt failed, with what it said.
    error TEXT,
    failed_at TIMESTAMPTZ,
    -- The one inbox item raised while held-back work waits on the repository.
    inbox_item_id UUID REFERENCES action_items (id) ON DELETE SET NULL,
    created_by UUID NOT NULL REFERENCES users (id),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT agent_configuration_status_singleton CHECK (singleton),
    CONSTRAINT agent_configuration_status_commit_shape CHECK (commit_sha ~ '^[0-9a-f]{40}$'),
    CONSTRAINT agent_configuration_status_fetched_together CHECK (
        (commit_sha IS NULL) = (fetched_at IS NULL)
        AND (commit_sha IS NULL) = (commit_subject IS NULL)
    ),
    CONSTRAINT agent_configuration_status_failed_together CHECK (
        (error IS NULL) = (failed_at IS NULL)
    ),
    CONSTRAINT agent_configuration_status_error_length CHECK (length(error) <= 4000),
    CONSTRAINT agent_configuration_status_skipped_array CHECK (jsonb_typeof(skipped) = 'array')
);

SELECT diesel_manage_updated_at('agent_configuration_status');

-- Nothing has been fetched yet; the engine fetches as it starts. Written as the system user.
INSERT INTO agent_configuration_status (created_by)
VALUES ('00000000-0000-0000-0000-000000000001');

-- The configuration a batch's worker ran on: `{ repository, commit, subject, fetchedAt, skipped }`,
-- recorded when its container fetched it.
ALTER TABLE work_batches
    ADD COLUMN configuration JSONB,
    ADD CONSTRAINT work_batches_configuration_object CHECK (jsonb_typeof(configuration) = 'object');

-- A worker that could not fetch the configuration repository before its first turn gives its
-- slot back: the batch waits in the queue again.
ALTER TABLE work_batch_events DROP CONSTRAINT work_batch_events_kind_known;
ALTER TABLE work_batch_events ADD CONSTRAINT work_batch_events_kind_known CHECK (kind IN (
    'submitted', 'held_for_approval', 'edited', 'approved', 'rejected', 'pinned', 'unpinned',
    'reprioritized', 'paused', 'resumed', 'cancelled', 'assigned', 'unassigned', 'started',
    'requeued', 'waiting', 'answered', 'finished'
));
