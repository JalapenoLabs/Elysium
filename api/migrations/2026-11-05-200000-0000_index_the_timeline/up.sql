-- Every timeline record is a source of the knowledge index: it is indexed in the transaction that
-- stores it, and the engine's backfill indexes the records stored before it was. See
-- docs/knowledge.md, Search.

-- How far the backfill of each source that needs one has got. A backfill walks a source's rows in
-- id order and indexes each, recording the last id after every batch, so an engine that stops part
-- way resumes where it left off. It walks to a version of how the source is cut into a document
-- (`INDEXED_AS` in code): a release that changes that bumps the version, and the next engine walks
-- every row again. One row per source; only the timeline needs one.
CREATE TABLE knowledge_backfills (
    source TEXT PRIMARY KEY CHECK (source IN ('timeline_record')),
    -- The version the walk indexes to; NULL before the first walk starts.
    version INTEGER CHECK (version > 0),
    -- The last row the walk indexed. Not a reference: that row may be deleted, and the walk still
    -- goes on from where it was.
    through_id UUID,
    -- When the walk reached the end; NULL while it is under way.
    finished_at TIMESTAMPTZ,
    created_by UUID NOT NULL REFERENCES users (id),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT knowledge_backfills_walk_has_a_version CHECK (
        version IS NOT NULL OR (through_id IS NULL AND finished_at IS NULL)
    )
);

SELECT diesel_manage_updated_at('knowledge_backfills');

INSERT INTO knowledge_backfills (source, created_by)
VALUES ('timeline_record', '00000000-0000-0000-0000-000000000001');

-- As the engine starts, the embedder forgets every embedding another model made. With the
-- timeline indexed there are passages by the hundred thousand, so that look is an index scan
-- rather than a pass over every passage. Passages still waiting for an embedding have no model
-- and are left out.
CREATE INDEX knowledge_passages_embedding_model ON knowledge_passages (embedding_model)
    WHERE embedding_model IS NOT NULL;
