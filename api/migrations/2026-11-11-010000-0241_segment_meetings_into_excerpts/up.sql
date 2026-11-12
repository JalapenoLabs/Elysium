-- Segmenting takes only what matters from a meeting: excerpts, each a range of its words and a
-- timeline record on the projects it is about, and black-outs, ranges that matter to nobody. See
-- docs/meeting-review.md, Excerpts and Black-outs, and docs/meetings.md, Segmentation.

-- An excerpt is a mark over its words (meeting_marks, kind 'excerpt') and the timeline record its
-- projects read. The record names its mark, as a turn summary's record names its turn: one record
-- per mark, and a mark is never deleted, only removed, so the reference always holds.
ALTER TABLE timeline_records
    ADD COLUMN excerpt_mark_id UUID UNIQUE REFERENCES meeting_marks (id);

-- Every topic a meeting was segmented into before becomes an excerpt over its words, keeping its
-- record, so its projects, its history, its clip, and its place in the index stay as they are.
-- A topic's span is on its meeting's clock (media's startMs and endMs), and its words are the
-- standing words that start inside it, so topics cut back to back share no word. A meeting in
-- words alone has every word and every topic at 0, so each of its topics covers it whole: its
-- topics were runs of lines the words no longer record. A topic no word starts inside, which only
-- a silence could be, covers the word said nearest its start. A topic whose meeting has no words
-- at all keeps its record and names no mark.
CREATE TEMPORARY TABLE excerpts_from_topics (
    mark_id UUID NOT NULL,
    record_id UUID NOT NULL,
    meeting_id UUID NOT NULL,
    start_word_id UUID NOT NULL,
    end_word_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

WITH topics AS (
    SELECT records.id AS record_id,
           records.created_at,
           (records.media ->> 'meetingId')::uuid AS meeting_id,
           (records.media ->> 'startMs')::bigint AS start_ms,
           (records.media ->> 'endMs')::bigint AS end_ms
    FROM timeline_records records
    WHERE records.source_kind = 'meeting'
      AND records.media ->> 'meetingId'
          ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
      AND records.media ->> 'startMs' ~ '^-?[0-9]+$'
      AND records.media ->> 'endMs' ~ '^-?[0-9]+$'
),
inside AS (
    SELECT topics.record_id, topics.created_at, topics.meeting_id, first.id AS start_word_id,
           last.id AS end_word_id
    FROM topics
    CROSS JOIN LATERAL (
        SELECT words.id
        FROM meeting_words words
        WHERE words.meeting_id = topics.meeting_id
          AND words.superseded_by IS NULL
          AND words.start_ms >= topics.start_ms
          AND (words.start_ms < topics.end_ms
               OR (topics.end_ms <= topics.start_ms AND words.start_ms = topics.start_ms))
        ORDER BY words.position ASC
        LIMIT 1
    ) first
    CROSS JOIN LATERAL (
        SELECT words.id
        FROM meeting_words words
        WHERE words.meeting_id = topics.meeting_id
          AND words.superseded_by IS NULL
          AND words.start_ms >= topics.start_ms
          AND (words.start_ms < topics.end_ms
               OR (topics.end_ms <= topics.start_ms AND words.start_ms = topics.start_ms))
        ORDER BY words.position DESC
        LIMIT 1
    ) last
),
nearest AS (
    SELECT topics.record_id, topics.created_at, topics.meeting_id, word.id AS start_word_id,
           word.id AS end_word_id
    FROM topics
    CROSS JOIN LATERAL (
        SELECT words.id
        FROM meeting_words words
        WHERE words.meeting_id = topics.meeting_id
          AND words.superseded_by IS NULL
        ORDER BY abs(words.start_ms - topics.start_ms), words.position
        LIMIT 1
    ) word
    WHERE topics.record_id NOT IN (SELECT record_id FROM inside)
)
INSERT INTO excerpts_from_topics (mark_id, record_id, meeting_id, start_word_id, end_word_id,
                                  created_at)
SELECT uuidv7(), record_id, meeting_id, start_word_id, end_word_id, created_at FROM inside
UNION ALL
SELECT uuidv7(), record_id, meeting_id, start_word_id, end_word_id, created_at FROM nearest;

-- Segmenting is Elysia's, so the topics she cut are her excerpts.
INSERT INTO meeting_marks (id, meeting_id, kind, start_word_id, end_word_id, data, created_by,
                           created_at)
SELECT mark_id, meeting_id, 'excerpt', start_word_id, end_word_id, '{}'::jsonb,
       '00000000-0000-0000-0000-000000000002', created_at
FROM excerpts_from_topics;

UPDATE timeline_records records
SET excerpt_mark_id = excerpts.mark_id
FROM excerpts_from_topics excerpts
WHERE records.id = excerpts.record_id;

DROP TABLE excerpts_from_topics;

-- A meeting's projects are its standing excerpts' records' projects, found through the marks, so
-- the index on the meeting id in a record's metadata reads nothing any more.
DROP INDEX timeline_records_meeting_segments;

-- A meeting's whole transcript is a source of the knowledge index, apart from its excerpts, so
-- what no excerpt took is still found (docs/knowledge.md, What is indexed): a column of its own,
-- unique and cascading, as every source is.
ALTER TABLE knowledge_documents
    ADD COLUMN meeting_id UUID UNIQUE REFERENCES meetings (id) ON DELETE CASCADE;

ALTER TABLE knowledge_documents DROP CONSTRAINT knowledge_documents_one_source;
ALTER TABLE knowledge_documents ADD CONSTRAINT knowledge_documents_one_source
    CHECK (num_nonnulls(timeline_record_id, page_id, meeting_id) = 1);

-- The meetings recorded before their transcripts were indexed are brought in by the knowledge
-- index's backfill, walked as a source of its own.
ALTER TABLE knowledge_backfills DROP CONSTRAINT knowledge_backfills_source_check;
ALTER TABLE knowledge_backfills ADD CONSTRAINT knowledge_backfills_source_check
    CHECK (source IN ('timeline_record', 'turn_summary', 'meeting'));

INSERT INTO knowledge_backfills (source, created_by)
VALUES ('meeting', '00000000-0000-0000-0000-000000000001');
