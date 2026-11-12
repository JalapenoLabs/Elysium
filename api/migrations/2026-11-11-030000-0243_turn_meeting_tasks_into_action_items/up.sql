-- The tasks heard in a meeting, each turned into action items: a project's through its Project
-- Manager, as its changeset under the project's level of autonomy, and everyone else's at once.
-- See docs/meetings.md, Tasks.

-- A task is a mark of kind `task` over the words that say it, which moves with every edit as any
-- mark does; what the task is lives here, beside its mark, as a thread's comments do.
CREATE TABLE meeting_tasks (
    mark_id UUID PRIMARY KEY REFERENCES meeting_marks (id) ON DELETE CASCADE,
    meeting_id UUID NOT NULL REFERENCES meetings (id) ON DELETE CASCADE,
    -- What is to be done, as a short phrase.
    title TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    -- Who owes it: a person on Elysium, someone else by the name the meeting gives them, or nobody
    -- named.
    owed_by_user_id UUID REFERENCES users (id),
    owed_by_name TEXT CHECK (char_length(owed_by_name) BETWEEN 1 AND 200),
    -- The project it is for, whose Project Manager makes its action items. A task on no project is
    -- the person's own, a coworker's, or Other, and has its item made at once.
    project_id UUID REFERENCES projects (id) ON DELETE SET NULL,
    -- Why the project's Project Manager made no action item of it, in its words.
    declined_reason TEXT CHECK (char_length(declined_reason) BETWEEN 1 AND 2000),
    declined_at TIMESTAMPTZ,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT meeting_tasks_owed_by_one CHECK (owed_by_user_id IS NULL OR owed_by_name IS NULL),
    CONSTRAINT meeting_tasks_declined_whole CHECK ((declined_reason IS NULL) = (declined_at IS NULL))
);

CREATE INDEX meeting_tasks_of_meeting ON meeting_tasks (meeting_id);
CREATE INDEX meeting_tasks_of_project ON meeting_tasks (project_id) WHERE project_id IS NOT NULL;

-- The action items a task was made into, so the bar along the meeting's top opens them and each
-- item's page opens the task's range.
CREATE TABLE meeting_task_items (
    action_item_id UUID PRIMARY KEY REFERENCES action_items (id) ON DELETE CASCADE,
    task_id UUID NOT NULL REFERENCES meeting_tasks (mark_id) ON DELETE CASCADE,
    -- Who it is for: whoever owes the task, or the meeting's person for one they wait on someone
    -- else for or nobody owes. Items are the whole workspace's until each person has a Next of their
    -- own, so this is whose it is. NULL for an item a project's Project Manager made.
    person_id UUID REFERENCES users (id),
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX meeting_task_items_of_task ON meeting_task_items (task_id);

-- A Project Manager's change names the task it came from, beside or instead of a record; a create
-- that names one makes an item of that task when it applies.
ALTER TABLE changeset_operations
    ADD COLUMN meeting_task_id UUID REFERENCES meeting_tasks (mark_id) ON DELETE SET NULL;

CREATE INDEX changeset_operations_of_meeting_task
    ON changeset_operations (meeting_task_id) WHERE meeting_task_id IS NOT NULL;

-- A task on a project wakes its Project Manager. The kind type is rebuilt rather than extended,
-- because a value added with ADD VALUE cannot be used in the same transaction, and the constraint
-- below names it.
ALTER TABLE project_manager_wakes DROP CONSTRAINT project_manager_wakes_kind_fields;
ALTER TYPE project_manager_wake_kind RENAME TO project_manager_wake_kind_without_task;
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
    'assignment',
    -- A task heard in a meeting is for the project, new or changed since it last read it.
    'task'
);
ALTER TABLE project_manager_wakes
    ALTER COLUMN kind TYPE project_manager_wake_kind
    USING kind::text::project_manager_wake_kind;
DROP TYPE project_manager_wake_kind_without_task;

ALTER TABLE project_manager_wakes
    ADD COLUMN meeting_task_id UUID REFERENCES meeting_tasks (mark_id) ON DELETE CASCADE;
ALTER TABLE project_manager_wakes ADD CONSTRAINT project_manager_wakes_kind_fields CHECK (
    (timeline_record_id IS NOT NULL) = (kind = 'record')
    AND (work_batch_id IS NOT NULL) = (kind = 'batch')
    AND (body IS NOT NULL) = (kind = 'message')
    AND (promise_id IS NOT NULL) = (kind = 'promise')
    AND (page_version IS NOT NULL) = (kind = 'brief')
    AND (action_item_id IS NOT NULL) = (kind = 'assignment')
    AND (meeting_task_id IS NOT NULL) = (kind = 'task')
);

-- A task waits once per project however often a meeting is read again meanwhile, so Done
-- reviewing reaches a Project Manager with each task that changed once.
CREATE UNIQUE INDEX project_manager_wakes_task_waits_once
    ON project_manager_wakes (project_id, meeting_task_id) WHERE turn_id IS NULL;
