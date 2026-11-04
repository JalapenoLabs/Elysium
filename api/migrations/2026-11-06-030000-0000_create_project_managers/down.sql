-- A Project Manager's acts read as Elysium's own once there are none, so the shapes that
-- name them go back to what they were.
UPDATE action_item_events SET actor = 'system' WHERE actor LIKE 'project_manager:%';
UPDATE action_item_comments SET author = 'system' WHERE author LIKE 'project_manager:%';
UPDATE changesets SET proposer = 'elysia' WHERE proposer LIKE 'project_manager:%';
UPDATE company_doc_proposals SET agent = NULL, agent_name = NULL WHERE agent LIKE 'project_manager:%';

ALTER TABLE action_item_events DROP CONSTRAINT action_item_events_actor_shape;
ALTER TABLE action_item_events ADD CONSTRAINT action_item_events_actor_shape CHECK (
    actor ~ '^(user|user:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|elysia|session:[1-9][0-9]*|watcher:[a-z][a-z0-9-]*|system)$'
);

ALTER TABLE action_item_comments DROP CONSTRAINT action_item_comments_author_shape;
ALTER TABLE action_item_comments ADD CONSTRAINT action_item_comments_author_shape CHECK (
    author ~ '^(user|user:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|elysia|session:[1-9][0-9]*|watcher:[a-z][a-z0-9-]*|system)$'
);

ALTER TABLE changesets DROP CONSTRAINT changesets_proposer_shape;
ALTER TABLE changesets ADD CONSTRAINT changesets_proposer_shape CHECK (proposer ~ '^(elysia|session:[1-9][0-9]*)$');

ALTER TABLE company_doc_proposals DROP CONSTRAINT company_doc_proposals_agent_check;
ALTER TABLE company_doc_proposals ADD CONSTRAINT company_doc_proposals_agent_check CHECK (
    agent ~ '^(mcp:[^[:space:]]{1,255}|session:[0-9]{1,18})$'
);

DROP TABLE project_manager_wakes;
DROP TABLE work_batches;
DROP TABLE project_manager_turns;
DROP TABLE project_managers;
DROP TYPE project_manager_wake_kind;
DROP TYPE work_batch_state;
DROP TYPE work_batch_priority;
DROP TYPE project_manager_turn_status;

-- What the Project Managers put on the timeline means nothing without them, and goes with
-- them: their records, with each record's attributions, history, and index entry.
DELETE FROM timeline_records WHERE created_by = '00000000-0000-0000-0000-000000000004';
DELETE FROM agent_tool_grants WHERE created_by = '00000000-0000-0000-0000-000000000004';

-- Everything else they caused (changesets, the items and comments those created, history,
-- company doc proposals) stays, as the system user's, so their machine user can go. Every
-- table names its creator in `created_by`, so each such column is moved over.
DO $$
DECLARE
    creator RECORD;
BEGIN
    FOR creator IN
        SELECT columns.table_name
        FROM information_schema.columns
        JOIN information_schema.tables USING (table_schema, table_name)
        WHERE columns.table_schema = 'public'
            AND columns.column_name = 'created_by'
            AND tables.table_type = 'BASE TABLE'
    LOOP
        EXECUTE format(
            'UPDATE %I SET created_by = %L WHERE created_by = %L',
            creator.table_name,
            '00000000-0000-0000-0000-000000000001',
            '00000000-0000-0000-0000-000000000004'
        );
    END LOOP;
END
$$;

DELETE FROM users WHERE id = '00000000-0000-0000-0000-000000000004';
