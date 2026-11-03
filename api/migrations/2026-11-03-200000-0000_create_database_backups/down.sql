ALTER TABLE action_item_comments DROP CONSTRAINT action_item_comments_author_shape;
ALTER TABLE action_item_comments ADD CONSTRAINT action_item_comments_author_shape CHECK (
    author ~ '^(user|user:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|elysia|session:[1-9][0-9]*|watcher:[a-z][a-z0-9-]*)$'
);

ALTER TABLE action_item_events DROP CONSTRAINT action_item_events_actor_shape;
ALTER TABLE action_item_events ADD CONSTRAINT action_item_events_actor_shape CHECK (
    actor ~ '^(user|user:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|elysia|session:[1-9][0-9]*|watcher:[a-z][a-z0-9-]*)$'
);

ALTER TABLE workspace_settings DROP COLUMN backup_storage_location_id;
DROP TABLE database_backups;
DROP TYPE database_backup_trigger;
DROP TYPE database_backup_status;
