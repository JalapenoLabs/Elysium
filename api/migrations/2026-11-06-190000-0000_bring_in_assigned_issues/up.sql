-- Every issue an issue connector (Jira, GitHub, YouTrack) brought in because it is assigned to
-- the connector's person, and the action item it raised, or the item it was already linked to.
-- See docs/action-items.md, Assigned issues.
--
-- One row per connector and issue is what keeps an issue from raising a second item: not when the
-- watcher reads it again on its next pass, nor after a restart, nor when two engines read at the
-- same moment, nor once its item was dismissed, deleted, or unlinked from it.
CREATE TABLE assigned_issues (
    id UUID PRIMARY KEY,
    -- Removing a connector removes its account and the links made through it, so what it brought
    -- in is forgotten with it.
    connector_id UUID NOT NULL REFERENCES connectors (id) ON DELETE CASCADE,
    -- The issue as its provider names it for good, as action_item_links.external_id holds it.
    external_id TEXT NOT NULL,
    action_item_id UUID NOT NULL REFERENCES action_items (id) ON DELETE CASCADE,
    -- The watcher's machine user, which brings every issue in.
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT assigned_issues_external_id_length CHECK (char_length(external_id) BETWEEN 1 AND 300),
    CONSTRAINT assigned_issues_once UNIQUE (connector_id, external_id)
);

-- The cascade from action_items.
CREATE INDEX assigned_issues_action_item_id_idx ON assigned_issues (action_item_id);

-- A GitHub connector chooses the repositories whose assigned issues come in, as YouTrack's chooses
-- its projects. One made before it could choose reads everything its token reaches, and keeps
-- doing so.
UPDATE connectors SET content = '{"repositories": "*"}'::jsonb WHERE kind = 'github';
