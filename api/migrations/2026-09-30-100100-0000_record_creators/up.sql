-- Every top-level row names the user who created it. See docs/auth.md.
--
-- Rows that predate accounts belong to the system user. The default only backfills them: it
-- is dropped straight after, so every insert must say who created the row.

ALTER TABLE projects ADD COLUMN created_by UUID NOT NULL
    DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES users (id);
ALTER TABLE projects ALTER COLUMN created_by DROP DEFAULT;

ALTER TABLE action_items ADD COLUMN created_by UUID NOT NULL
    DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES users (id);
ALTER TABLE action_items ALTER COLUMN created_by DROP DEFAULT;

ALTER TABLE initiatives ADD COLUMN created_by UUID NOT NULL
    DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES users (id);
ALTER TABLE initiatives ALTER COLUMN created_by DROP DEFAULT;

ALTER TABLE coding_sessions ADD COLUMN created_by UUID NOT NULL
    DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES users (id);
ALTER TABLE coding_sessions ALTER COLUMN created_by DROP DEFAULT;

ALTER TABLE satellites ADD COLUMN created_by UUID NOT NULL
    DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES users (id);
ALTER TABLE satellites ALTER COLUMN created_by DROP DEFAULT;

ALTER TABLE llms ADD COLUMN created_by UUID NOT NULL
    DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES users (id);
ALTER TABLE llms ALTER COLUMN created_by DROP DEFAULT;

ALTER TABLE github_credentials ADD COLUMN created_by UUID NOT NULL
    DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES users (id);
ALTER TABLE github_credentials ALTER COLUMN created_by DROP DEFAULT;

ALTER TABLE jira_credentials ADD COLUMN created_by UUID NOT NULL
    DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES users (id);
ALTER TABLE jira_credentials ALTER COLUMN created_by DROP DEFAULT;

ALTER TABLE storage_locations ADD COLUMN created_by UUID NOT NULL
    DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES users (id);
ALTER TABLE storage_locations ALTER COLUMN created_by DROP DEFAULT;

ALTER TABLE environment_variables ADD COLUMN created_by UUID NOT NULL
    DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES users (id);
ALTER TABLE environment_variables ALTER COLUMN created_by DROP DEFAULT;

ALTER TABLE mail_servers ADD COLUMN created_by UUID NOT NULL
    DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES users (id);
ALTER TABLE mail_servers ALTER COLUMN created_by DROP DEFAULT;

ALTER TABLE mail_domains ADD COLUMN created_by UUID NOT NULL
    DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES users (id);
ALTER TABLE mail_domains ALTER COLUMN created_by DROP DEFAULT;

ALTER TABLE mail_accounts ADD COLUMN created_by UUID NOT NULL
    DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES users (id);
ALTER TABLE mail_accounts ALTER COLUMN created_by DROP DEFAULT;

-- A changeset is created by the machine that proposed it, and decided by the person who
-- approved or rejected it.
ALTER TABLE changesets ADD COLUMN created_by UUID NOT NULL
    DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES users (id);
ALTER TABLE changesets ALTER COLUMN created_by DROP DEFAULT;

ALTER TABLE changesets ADD COLUMN decided_by UUID REFERENCES users (id);
UPDATE changesets SET decided_by = '00000000-0000-0000-0000-000000000001' WHERE decided_at IS NOT NULL;
ALTER TABLE changesets ADD CONSTRAINT changesets_decided_by_set
    CHECK ((decided_by IS NULL) = (decided_at IS NULL));

-- The two tables that lacked it.
ALTER TABLE mail_servers ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT now();
SELECT diesel_manage_updated_at('mail_servers');
ALTER TABLE mail_domains ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT now();
SELECT diesel_manage_updated_at('mail_domains');

-- History and comments name a person as user:<id>. A bare `user` is the one person who used
-- Elysium before accounts, and stays valid for the rows they wrote.
ALTER TABLE action_item_events DROP CONSTRAINT action_item_events_actor_shape;
ALTER TABLE action_item_events ADD CONSTRAINT action_item_events_actor_shape CHECK (
    actor ~ '^(user|user:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|elysia|session:[1-9][0-9]*|watcher:[a-z][a-z0-9-]*)$'
);

ALTER TABLE action_item_comments DROP CONSTRAINT action_item_comments_author_shape;
ALTER TABLE action_item_comments ADD CONSTRAINT action_item_comments_author_shape CHECK (
    author ~ '^(user|user:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|elysia|session:[1-9][0-9]*|watcher:[a-z][a-z0-9-]*)$'
);
