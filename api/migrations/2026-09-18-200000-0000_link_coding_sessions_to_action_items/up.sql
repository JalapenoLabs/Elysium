-- A coding session can be started from an action item; see docs/action-items.md. The link
-- is a pointer for showing the item on the session and the item's sessions on the item.
--
-- Items are deleted softly, so a session keeps its link while its item is hidden. Should an
-- item row ever go for good, the session outlives it: SET NULL drops only the pointer.

ALTER TABLE coding_sessions
    ADD COLUMN action_item_id UUID REFERENCES action_items (id) ON DELETE SET NULL;

-- Serves an item's sessions, and the SET NULL when an item row is deleted.
CREATE INDEX coding_sessions_action_item_id_idx ON coding_sessions (action_item_id);
