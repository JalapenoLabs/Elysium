-- A person assigns an action item to its lead project's Project Manager, which wakes it to plan the
-- item. See docs/action-items.md, Assigning to a Project Manager.

-- An item is assigned to its lead project's Project Manager, never another's, so the assignment
-- names no project of its own: moving the lead ends it, in the code that moves the lead. Who
-- assigned it is the person who chose to, also when they accepted Elysia's suggestion.
--
-- No check ties an assignment to a lead: taking an item out of its lead project nulls the lead at
-- once (`action_items_lead_project_is_a_membership`, ON DELETE SET NULL), before the code moving
-- the lead in the same transaction ends the assignment, and a check cannot wait for the commit.
ALTER TABLE action_items
    ADD COLUMN manager_assigned_at TIMESTAMPTZ,
    ADD COLUMN manager_assigned_by UUID REFERENCES users (id),
    ADD CONSTRAINT action_items_manager_assignment_whole
        CHECK ((manager_assigned_at IS NULL) = (manager_assigned_by IS NULL));

-- Assigning wakes the lead project's Project Manager. The kind type is rebuilt rather than
-- extended, because a value added with ADD VALUE cannot be used in the same transaction, and the
-- constraint below names it.
ALTER TABLE project_manager_wakes DROP CONSTRAINT project_manager_wakes_kind_fields;
ALTER TYPE project_manager_wake_kind RENAME TO project_manager_wake_kind_without_assignment;
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
    -- A person assigned it an action item the project leads, to plan.
    'assignment'
);
ALTER TABLE project_manager_wakes
    ALTER COLUMN kind TYPE project_manager_wake_kind
    USING kind::text::project_manager_wake_kind;
DROP TYPE project_manager_wake_kind_without_assignment;

ALTER TABLE project_manager_wakes
    ADD COLUMN action_item_id UUID REFERENCES action_items (id) ON DELETE CASCADE;
ALTER TABLE project_manager_wakes ADD CONSTRAINT project_manager_wakes_kind_fields CHECK (
    (timeline_record_id IS NOT NULL) = (kind = 'record')
    AND (work_batch_id IS NOT NULL) = (kind = 'batch')
    AND (body IS NOT NULL) = (kind = 'message')
    AND (promise_id IS NOT NULL) = (kind = 'promise')
    AND (action_item_id IS NOT NULL) = (kind = 'assignment')
);

-- An item waits once per project however often it is assigned meanwhile.
CREATE UNIQUE INDEX project_manager_wakes_assignment_waits_once
    ON project_manager_wakes (project_id, action_item_id) WHERE turn_id IS NULL;

-- An item's latest assignment wake, for where its plan stands.
CREATE INDEX project_manager_wakes_by_action_item
    ON project_manager_wakes (action_item_id, created_at DESC) WHERE action_item_id IS NOT NULL;
