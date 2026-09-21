DROP TABLE jira_done_transitions;
DROP TABLE link_watch_cursors;
DROP TABLE action_item_link_writes;
-- Dropping the column drops its index and foreign key with it.
ALTER TABLE initiative_items DROP COLUMN via_link_id;
DROP TABLE initiative_links;
DROP TABLE action_item_links;
DROP TYPE link_write_kind;
DROP TYPE link_state;
DROP TYPE container_kind;
DROP TYPE link_kind;
DROP TYPE link_provider;
