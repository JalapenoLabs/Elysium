-- The history of what timeline records are attributed to, one row per change: Elysia attributing
-- a meeting's segment or a conversation's message as segmenting reads it, and a person
-- confirming, adding, removing, or undoing one. Shaped like learning_events, so any entry can be
-- undone and the undo is an entry of its own. See docs/connectors.md, The timeline.
CREATE TABLE timeline_record_events (
    -- UUIDv7, so entries order as they were written.
    id UUID PRIMARY KEY,
    timeline_record_id UUID NOT NULL REFERENCES timeline_records (id) ON DELETE CASCADE,
    -- The project the change was about. Removing the project removes its attributions, so its
    -- history goes with them.
    project_id UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    -- `attributed` (added, or how sure it is changed) or `unattributed` (removed). Text rather
    -- than an enum, as for learning_events, so a later kind needs no enum migration.
    kind TEXT NOT NULL CHECK (kind ~ '^[a-z][a-z_]{0,63}$'),
    -- Who made the change: a person, or Elysia's machine user for what segmenting attributes.
    created_by UUID NOT NULL REFERENCES users (id),
    -- `{ before, after }`, each `{ confidence, attributedBy }` or null where there was none, and
    -- `undid`, the entry an undo reversed.
    data JSONB NOT NULL CHECK (jsonb_typeof(data) = 'object'),
    created_at TIMESTAMPTZ NOT NULL
);

-- One record's history, in order.
CREATE INDEX timeline_record_events_record ON timeline_record_events (timeline_record_id, id);
-- Whether a person decided a record's attribution to a project, which Elysia never overrides.
CREATE INDEX timeline_record_events_pair ON timeline_record_events (timeline_record_id, project_id);
