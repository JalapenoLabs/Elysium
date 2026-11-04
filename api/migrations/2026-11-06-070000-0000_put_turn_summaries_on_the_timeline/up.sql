-- Every turn summary of a session that has a project is a timeline record of that project, so it
-- reads beside the meetings and messages about the project and is indexed for search like every
-- record. See docs/coding.md, Turn summaries, and docs/knowledge.md, Timeline records.

-- A summary's record names the summary it is. That is what makes it one record per turn, however
-- often it is written (as the summary is, and again by the backfill below), and what takes it
-- away with its summary when its session is deleted, as the session's other history goes.
ALTER TABLE timeline_records
    ADD COLUMN turn_summary_session_id BIGINT,
    ADD COLUMN turn_summary_turn_id UUID,
    ADD CONSTRAINT timeline_records_turn_summary_fkey
        FOREIGN KEY (turn_summary_session_id, turn_summary_turn_id)
        REFERENCES turn_summaries (session_id, turn_id) ON DELETE CASCADE,
    ADD CONSTRAINT timeline_records_one_per_turn_summary
        UNIQUE (turn_summary_session_id, turn_summary_turn_id),
    -- Both or neither, and only on a summary's record, so no record half-names a summary and no
    -- other kind of record is deleted with one.
    ADD CONSTRAINT timeline_records_turn_summary_kind CHECK (
        (turn_summary_session_id IS NULL) = (turn_summary_turn_id IS NULL)
        AND (turn_summary_turn_id IS NOT NULL) = (source_kind = 'turn_summary')
    );

-- The backfill walks summaries in turn order, a batch at a time.
CREATE INDEX turn_summaries_turn_id ON turn_summaries (turn_id);

-- The summaries written before this are put on the timeline by the knowledge index's backfill,
-- walked as a source of its own.
ALTER TABLE knowledge_backfills DROP CONSTRAINT knowledge_backfills_source_check;
ALTER TABLE knowledge_backfills ADD CONSTRAINT knowledge_backfills_source_check
    CHECK (source IN ('timeline_record', 'turn_summary'));

INSERT INTO knowledge_backfills (source, created_by)
VALUES ('turn_summary', '00000000-0000-0000-0000-000000000001');
