-- Dropping the column drops its index and foreign key with it.
ALTER TABLE action_item_events DROP COLUMN changeset_id;
DROP TABLE changeset_operations;
DROP TABLE changesets;
DROP TYPE changeset_outcome;
DROP TYPE changeset_decision;
DROP TYPE changeset_state;
