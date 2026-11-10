-- Without assignments their wakes mean nothing, so they go, and the kind type is rebuilt without
-- theirs: Postgres cannot drop an enum value.
DROP INDEX project_manager_wakes_by_action_item;
DROP INDEX project_manager_wakes_assignment_waits_once;
DELETE FROM project_manager_wakes WHERE kind = 'assignment';
ALTER TABLE project_manager_wakes DROP CONSTRAINT project_manager_wakes_kind_fields;
ALTER TABLE project_manager_wakes DROP COLUMN action_item_id;

ALTER TYPE project_manager_wake_kind RENAME TO project_manager_wake_kind_with_assignment;
CREATE TYPE project_manager_wake_kind AS ENUM (
    -- A record was attributed to the project: a meeting's segment, a conversation's message,
    -- or a record a person put on it.
    'record',
    -- A batch it submitted finished.
    'batch',
    -- A person or Elysia wrote to it.
    'message',
    -- A promise was found on the project, or the work it waits on finished.
    'promise'
);
ALTER TABLE project_manager_wakes
    ALTER COLUMN kind TYPE project_manager_wake_kind
    USING kind::text::project_manager_wake_kind;
DROP TYPE project_manager_wake_kind_with_assignment;

ALTER TABLE project_manager_wakes ADD CONSTRAINT project_manager_wakes_kind_fields CHECK (
    (timeline_record_id IS NOT NULL) = (kind = 'record')
    AND (work_batch_id IS NOT NULL) = (kind = 'batch')
    AND (body IS NOT NULL) = (kind = 'message')
    AND (promise_id IS NOT NULL) = (kind = 'promise')
);

ALTER TABLE action_items
    DROP CONSTRAINT action_items_manager_assignment_whole,
    DROP COLUMN manager_assigned_by,
    DROP COLUMN manager_assigned_at;
