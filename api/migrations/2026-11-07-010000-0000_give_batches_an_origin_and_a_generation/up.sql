-- Every batch has an origin and a generation, worked out from its provenance as it is submitted,
-- so work that files work is held back structurally. See docs/autonomy.md, Origins and
-- generations, and docs/engine.md, Origins and generations.

CREATE TYPE work_batch_origin AS ENUM (
    -- It traces to a person: their message, an issue assigned to them, a meeting, or a
    -- coworker's request.
    'human_rooted',
    -- It traces only to what an agent noticed.
    'found'
);

ALTER TABLE work_batches
    ADD COLUMN origin work_batch_origin,
    -- How many batches led to it: 0 for work a person called for, one more than its deepest
    -- parent for work planned from what a worker did. The limit is a code constant
    -- (`work_batch::lineage::MAX_GENERATION`), so it is not repeated here.
    ADD COLUMN generation INTEGER NOT NULL DEFAULT 0
        CONSTRAINT work_batches_generation_counts CHECK (generation >= 0),
    -- The batches it was planned from, as an array of ids.
    ADD COLUMN parent_batch_ids JSONB NOT NULL DEFAULT '[]'
        CONSTRAINT work_batches_parent_batch_ids_array CHECK (jsonb_typeof(parent_batch_ids) = 'array'),
    -- A person's (or Elysia's) messages to its Project Manager that called for it, as an array of
    -- the ids of their `project_manager_wakes`.
    ADD COLUMN source_message_ids JSONB NOT NULL DEFAULT '[]'
        CONSTRAINT work_batches_source_message_ids_array CHECK (jsonb_typeof(source_message_ids) = 'array');

-- Existing batches are given the origin their provenance proves, by the rules the API applies to
-- every new one (`work_batch::lineage`): a record a person owns, or an action item a person or a
-- provider's watcher created, a person approved creating, or a person accepted from the inbox.
-- Their generation stays 0 and their parents empty: they were planned before chains were
-- recorded, so a chain counts from them.
UPDATE work_batches AS batch
SET origin = CASE
    WHEN EXISTS (
        SELECT 1
        FROM jsonb_array_elements_text(batch.source_record_ids) AS named (id)
        JOIN timeline_records AS record ON record.id = named.id::uuid
        JOIN users AS owner ON owner.id = record.created_by
        WHERE owner.role IN ('admin', 'member', 'guest')
    ) OR EXISTS (
        SELECT 1
        FROM jsonb_array_elements_text(batch.action_item_ids) AS named (id)
        JOIN action_item_events AS event ON event.action_item_id = named.id::uuid
        LEFT JOIN changesets AS changeset ON changeset.id = event.changeset_id
        LEFT JOIN users AS decider ON decider.id = changeset.decided_by
        WHERE (
            event.kind = 'created'
            AND event.changeset_id IS NULL
            AND (event.actor LIKE 'user%' OR event.actor LIKE 'watcher:%')
        ) OR (
            event.kind = 'created'
            AND decider.role IN ('admin', 'member', 'guest')
        ) OR (
            event.kind = 'state_changed'
            AND event.actor LIKE 'user%'
            AND event.data ->> 'from' = 'inbox'
            AND event.data ->> 'to' = 'open'
        )
    ) THEN 'human_rooted'::work_batch_origin
    ELSE 'found'::work_batch_origin
END;

-- From here every batch names its origin and generation as it is stored: nothing defaults.
ALTER TABLE work_batches
    ALTER COLUMN origin SET NOT NULL,
    ALTER COLUMN generation DROP DEFAULT;
