-- Without tasks their wakes, their links to changes and items, and their marks mean nothing, so
-- they go, and the wake kind type is rebuilt without theirs: Postgres cannot drop an enum value.
DROP INDEX project_manager_wakes_task_waits_once;
DELETE FROM project_manager_wakes WHERE kind = 'task';
ALTER TABLE project_manager_wakes DROP CONSTRAINT project_manager_wakes_kind_fields;
ALTER TABLE project_manager_wakes DROP COLUMN meeting_task_id;

ALTER TYPE project_manager_wake_kind RENAME TO project_manager_wake_kind_with_task;
CREATE TYPE project_manager_wake_kind AS ENUM (
    -- A record was attributed to the project: a meeting's segment, a conversation's message,
    -- or a record a person put on it.
    'record',
    -- A batch it submitted finished.
    'batch',
    -- A person or Elysia wrote to it.
    'message',
    -- A promise was found on the project, or the work it waits on finished.
    'promise',
    -- Someone else published the project's brief.
    'brief',
    -- A person assigned it an action item the project leads, to plan.
    'assignment'
);
ALTER TABLE project_manager_wakes
    ALTER COLUMN kind TYPE project_manager_wake_kind
    USING kind::text::project_manager_wake_kind;
DROP TYPE project_manager_wake_kind_with_task;

ALTER TABLE project_manager_wakes ADD CONSTRAINT project_manager_wakes_kind_fields CHECK (
    (timeline_record_id IS NOT NULL) = (kind = 'record')
    AND (work_batch_id IS NOT NULL) = (kind = 'batch')
    AND (body IS NOT NULL) = (kind = 'message')
    AND (promise_id IS NOT NULL) = (kind = 'promise')
    AND (page_version IS NOT NULL) = (kind = 'brief')
    AND (action_item_id IS NOT NULL) = (kind = 'assignment')
);

DROP INDEX changeset_operations_of_meeting_task;
ALTER TABLE changeset_operations DROP COLUMN meeting_task_id;

DROP TABLE meeting_task_items;
DROP TABLE meeting_tasks;
DELETE FROM meeting_marks WHERE kind = 'task';
