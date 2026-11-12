DELETE FROM knowledge_backfills WHERE source = 'meeting';
ALTER TABLE knowledge_backfills DROP CONSTRAINT knowledge_backfills_source_check;
ALTER TABLE knowledge_backfills ADD CONSTRAINT knowledge_backfills_source_check
    CHECK (source IN ('timeline_record', 'turn_summary'));

-- Meetings' transcripts are indexed only through this migration; their passages go with them.
DELETE FROM knowledge_documents WHERE meeting_id IS NOT NULL;
ALTER TABLE knowledge_documents DROP CONSTRAINT knowledge_documents_one_source;
ALTER TABLE knowledge_documents DROP COLUMN meeting_id;
ALTER TABLE knowledge_documents ADD CONSTRAINT knowledge_documents_one_source
    CHECK (num_nonnulls(timeline_record_id, page_id) = 1);

CREATE INDEX timeline_records_meeting_segments ON timeline_records ((metadata ->> 'meetingId'))
    WHERE source_kind = 'meeting';

-- Every excerpt's record is a segment record again, as it was before; the marks that made them
-- excerpts exist only through this migration.
CREATE TEMPORARY TABLE excerpt_marks AS
SELECT excerpt_mark_id AS id FROM timeline_records WHERE excerpt_mark_id IS NOT NULL;

ALTER TABLE timeline_records DROP COLUMN excerpt_mark_id;
DELETE FROM meeting_marks WHERE id IN (SELECT id FROM excerpt_marks);
DROP TABLE excerpt_marks;
