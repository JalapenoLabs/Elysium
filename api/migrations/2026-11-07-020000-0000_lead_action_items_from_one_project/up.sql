-- Who put an action item in each of its projects and how sure they were, and the one project that
-- leads it, whose Project Manager alone may submit work for it. See docs/action-items.md, Projects
-- and the lead.

-- An attribution step guesses an arrived item's projects with a confidence; everyone else's choice
-- is certain. `attributed_by` is the actor, as history names it (`user:<id>`, `elysia`, ...).
ALTER TABLE action_item_projects
    ADD COLUMN confidence REAL NOT NULL DEFAULT 1,
    ADD COLUMN attributed_by TEXT;

-- Each membership so far was made by whoever its history says: the latest `project_added` for the
-- pair, or the item's creation when it was created in the project.
UPDATE action_item_projects membership
SET attributed_by = COALESCE(
    (
        SELECT event.actor
        FROM action_item_events event
        WHERE event.action_item_id = membership.action_item_id
          AND event.kind = 'project_added'
          AND event.data ->> 'projectId' = membership.project_id::text
        ORDER BY event.id DESC
        LIMIT 1
    ),
    (
        SELECT event.actor
        FROM action_item_events event
        WHERE event.action_item_id = membership.action_item_id
          AND event.kind = 'created'
        ORDER BY event.id
        LIMIT 1
    ),
    'system'
);

ALTER TABLE action_item_projects
    ALTER COLUMN confidence DROP DEFAULT,
    ALTER COLUMN attributed_by SET NOT NULL,
    ADD CONSTRAINT action_item_projects_confidence_range CHECK (confidence BETWEEN 0 AND 1),
    ADD CONSTRAINT action_item_projects_attributed_by_length
        CHECK (char_length(attributed_by) BETWEEN 1 AND 100);

-- The lead is always one of the item's projects. The code moves it, recording the change, before a
-- membership goes; the foreign key only keeps a stray delete from leaving it pointing nowhere. It is
-- checked at commit, so an item is created with its lead in the same statement as its row, before
-- its memberships.
ALTER TABLE action_items
    ADD COLUMN lead_project_id UUID,
    ADD CONSTRAINT action_items_lead_project_is_a_membership
        FOREIGN KEY (id, lead_project_id)
        REFERENCES action_item_projects (action_item_id, project_id)
        ON DELETE SET NULL (lead_project_id)
        DEFERRABLE INITIALLY DEFERRED;

-- Every item already in a project is led by the one it joined first, the rule the code keeps for an
-- item's first project. Backfilling is not an edit, so `updated_at` stays as it was.
ALTER TABLE action_items DISABLE TRIGGER set_updated_at;
UPDATE action_items item
SET lead_project_id = (
    SELECT membership.project_id
    FROM action_item_projects membership
    WHERE membership.action_item_id = item.id
    ORDER BY membership.created_at, membership.project_id
    LIMIT 1
);
ALTER TABLE action_items ENABLE TRIGGER set_updated_at;
