-- Rows written by people since are attributed to the one user there was before accounts.
UPDATE action_item_comments SET author = 'user' WHERE author LIKE 'user:%';
ALTER TABLE action_item_comments DROP CONSTRAINT action_item_comments_author_shape;
ALTER TABLE action_item_comments ADD CONSTRAINT action_item_comments_author_shape
    CHECK (author ~ '^(user|elysia|session:[1-9][0-9]*|watcher:[a-z][a-z0-9-]*)$');

UPDATE action_item_events SET actor = 'user' WHERE actor LIKE 'user:%';
ALTER TABLE action_item_events DROP CONSTRAINT action_item_events_actor_shape;
ALTER TABLE action_item_events ADD CONSTRAINT action_item_events_actor_shape
    CHECK (actor ~ '^(user|elysia|session:[1-9][0-9]*|watcher:[a-z][a-z0-9-]*)$');

DROP TRIGGER set_updated_at ON mail_domains;
ALTER TABLE mail_domains DROP COLUMN updated_at;
DROP TRIGGER set_updated_at ON mail_servers;
ALTER TABLE mail_servers DROP COLUMN updated_at;

ALTER TABLE changesets DROP CONSTRAINT changesets_decided_by_set;
ALTER TABLE changesets DROP COLUMN decided_by;
ALTER TABLE changesets DROP COLUMN created_by;
ALTER TABLE mail_accounts DROP COLUMN created_by;
ALTER TABLE mail_domains DROP COLUMN created_by;
ALTER TABLE mail_servers DROP COLUMN created_by;
ALTER TABLE environment_variables DROP COLUMN created_by;
ALTER TABLE storage_locations DROP COLUMN created_by;
ALTER TABLE jira_credentials DROP COLUMN created_by;
ALTER TABLE github_credentials DROP COLUMN created_by;
ALTER TABLE llms DROP COLUMN created_by;
ALTER TABLE satellites DROP COLUMN created_by;
ALTER TABLE coding_sessions DROP COLUMN created_by;
ALTER TABLE initiatives DROP COLUMN created_by;
ALTER TABLE action_items DROP COLUMN created_by;
ALTER TABLE projects DROP COLUMN created_by;
