-- Promises a person made in a conversation ("I'll let you know here when it's live"), recorded on
-- the project whose work keeps them, and the follow-up their Project Manager drafts once that work
-- is done. See docs/engine.md, Follow-ups.

CREATE TYPE promise_state AS ENUM (
    -- Recorded; its work has not finished, or it is tied to no work yet.
    'waiting',
    -- Its work finished; its Project Manager owes the follow-up.
    'due',
    -- The follow-up is drafted, as an item the person sends, edits, or dismisses.
    'drafted',
    -- The person sent the follow-up, or resolved its item.
    'kept',
    -- The person dismissed the follow-up.
    'dismissed'
);

CREATE TABLE promises (
    -- UUIDv7, so promises order as they were found.
    id UUID PRIMARY KEY,
    -- The project whose Project Manager watches its work and drafts its follow-up.
    project_id UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    -- The person's message it was made in, which its follow-up answers in the same conversation.
    -- One promise per message, so reading a conversation again as it grows records it once.
    timeline_record_id UUID NOT NULL UNIQUE REFERENCES timeline_records (id) ON DELETE CASCADE,
    -- Who it is for, by name, as the conversation names them.
    recipient TEXT NOT NULL,
    -- What was promised, as a short phrase ("when the login fix is live").
    summary TEXT NOT NULL,
    state promise_state NOT NULL DEFAULT 'waiting',
    -- The work it waits on, at most one: a batch, or an action item, which stands for whatever
    -- is linked to it (a tracker issue closing, a pull request merging). Its Project Manager
    -- ties it; unset until it does.
    work_batch_id UUID REFERENCES work_batches (id) ON DELETE SET NULL,
    work_item_id UUID REFERENCES action_items (id) ON DELETE SET NULL,
    -- When its work finished.
    due_at TIMESTAMPTZ,
    -- The item its follow-up is, once drafted.
    follow_up_item_id UUID REFERENCES action_items (id) ON DELETE SET NULL,
    -- Elysia, who found it.
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT promises_recipient_length CHECK (char_length(recipient) BETWEEN 1 AND 200),
    CONSTRAINT promises_summary_length CHECK (char_length(summary) BETWEEN 1 AND 500),
    CONSTRAINT promises_one_work CHECK (num_nonnulls(work_batch_id, work_item_id) <= 1),
    -- A promise leaves `waiting` only when its work finishes, and never returns to it.
    CONSTRAINT promises_due_at_set CHECK ((due_at IS NULL) = (state = 'waiting'))
);

SELECT diesel_manage_updated_at('promises');

-- A project's promises, newest first.
CREATE INDEX promises_by_project ON promises (project_id, id DESC);
-- What a batch finishing or an item resolving makes due, and what a follow-up item's state moves.
CREATE INDEX promises_by_batch ON promises (work_batch_id) WHERE work_batch_id IS NOT NULL;
CREATE INDEX promises_by_work_item ON promises (work_item_id) WHERE work_item_id IS NOT NULL;
CREATE INDEX promises_by_follow_up ON promises (follow_up_item_id) WHERE follow_up_item_id IS NOT NULL;

-- The follow-up a Project Manager drafted for a promise, written for its person. Sending it keeps
-- the promise; it never shows in the reply box of the record it answers, which is the person's own.
ALTER TABLE drafts ADD COLUMN promise_id UUID REFERENCES promises (id) ON DELETE SET NULL;
CREATE UNIQUE INDEX drafts_one_per_promise ON drafts (promise_id) WHERE promise_id IS NOT NULL;

-- A promise wakes its project's Project Manager when it is found and when its work finishes. The
-- kind type is rebuilt rather than extended, because a value added with ADD VALUE cannot be used
-- in the same transaction, and the constraint below names it.
ALTER TABLE project_manager_wakes DROP CONSTRAINT project_manager_wakes_kind_fields;
ALTER TYPE project_manager_wake_kind RENAME TO project_manager_wake_kind_without_promise;
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
DROP TYPE project_manager_wake_kind_without_promise;

ALTER TABLE project_manager_wakes
    ADD COLUMN promise_id UUID REFERENCES promises (id) ON DELETE CASCADE;
ALTER TABLE project_manager_wakes ADD CONSTRAINT project_manager_wakes_kind_fields CHECK (
    (timeline_record_id IS NOT NULL) = (kind = 'record')
    AND (work_batch_id IS NOT NULL) = (kind = 'batch')
    AND (body IS NOT NULL) = (kind = 'message')
    AND (promise_id IS NOT NULL) = (kind = 'promise')
);

-- A promise waits once per project however often it is woken for meanwhile.
CREATE UNIQUE INDEX project_manager_wakes_promise_waits_once
    ON project_manager_wakes (project_id, promise_id) WHERE turn_id IS NULL;
