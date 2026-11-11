-- Comments on a meeting: threads on ranges of its final transcript (docs/meeting-review.md,
-- Comments). A thread is a mark of kind `comment` over its range, which draws it, moves with an
-- edit as every mark does, and goes and comes back with its own history entries; these tables
-- hold what was written in it and who it told.

-- What was written in a thread, oldest first.
CREATE TABLE meeting_comments (
    -- UUIDv7, so a thread's comments order as they were written.
    id UUID PRIMARY KEY,
    -- The thread: its comment mark. Removing the mark keeps its comments, so an undo puts the
    -- thread back whole.
    mark_id UUID NOT NULL REFERENCES meeting_marks (id) ON DELETE CASCADE,
    -- Plain text, as written. The longest is `meeting_comment::BODY_MAX_CHARACTERS`.
    body TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 20000 AND btrim(body) <> ''),
    -- Who wrote it: a person, or Elysia answering in the thread.
    created_by UUID NOT NULL REFERENCES users (id),
    -- The agent acting for them, as on `meeting_marks`.
    agent TEXT CHECK (char_length(agent) BETWEEN 1 AND 300),
    agent_name TEXT CHECK (char_length(agent_name) BETWEEN 1 AND 200),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX meeting_comments_of_thread ON meeting_comments (mark_id, id);

-- Who a comment mentions: each person is told with an inbox item, and Elysia answers in the
-- thread. Its author is the comment's.
CREATE TABLE meeting_comment_mentions (
    comment_id UUID NOT NULL REFERENCES meeting_comments (id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users (id),
    PRIMARY KEY (comment_id, user_id)
);

-- The inbox items a thread raised, one for each person a comment, a reply, a resolve, or a
-- reopen told, so an item's page opens the thread it is about.
CREATE TABLE meeting_thread_items (
    action_item_id UUID PRIMARY KEY REFERENCES action_items (id) ON DELETE CASCADE,
    mark_id UUID NOT NULL REFERENCES meeting_marks (id) ON DELETE CASCADE,
    -- Who it tells: someone mentioned, or someone taking part in the thread. Items are the whole
    -- workspace's until each person has a Next of their own, so this is who it is for.
    person_id UUID NOT NULL REFERENCES users (id),
    -- Whose change raised it.
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
