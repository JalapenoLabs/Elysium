-- A meeting is the one event that happened; a capture is one record of it, a connector's meeting
-- or (later) an upload. A meeting has one or more captures, and every capture belongs to one
-- meeting. The final transcript is a stream of words with stable ids, on the meeting's clock, and
-- every mark on a meeting is a range of word ids. Every change to a meeting is in its history.
-- See docs/meeting-review.md and docs/meetings.md.
--
-- Every meeting today becomes a meeting with one capture holding what it was fetched with, and its
-- final transcript is built from its corrected copy, or from the transcript it was segmented from
-- when it was never corrected. Nothing is fetched, transcribed, corrected, or segmented again.

-- What its source gave, kept as it came and never written after its fetch: the transcript and the
-- recording, with the hash of each. `UNIQUE (connector_id, external_id)` moves here from
-- `meetings`, so a connector records each of its meetings once.
CREATE TABLE meeting_captures (
    id UUID PRIMARY KEY,
    meeting_id UUID NOT NULL REFERENCES meetings (id) ON DELETE CASCADE,
    -- The connector it was read through. Kept when the connector is removed, so the meeting stays.
    -- NULL for an upload.
    connector_id UUID REFERENCES connectors (id) ON DELETE SET NULL,
    -- Which source it came from, such as `fireflies`.
    source_kind TEXT NOT NULL CHECK (source_kind ~ '^[a-z][a-z0-9_]{0,31}$'),
    -- The source's own id for it, unique per connector. NULL for an upload.
    external_id TEXT CHECK (char_length(external_id) BETWEEN 1 AND 1024),
    -- Its page at the source.
    url TEXT CHECK (char_length(url) <= 4096),
    -- The timestamped transcript as the source sent it: an array of
    -- `{ startMs, endMs, speaker, text }`, times from the start of its own recording.
    transcript JSONB NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(transcript) = 'array'),
    -- The SHA-256 of `transcript` as it was stored, as lowercase hex. NULL until it is fetched.
    transcript_hash TEXT CHECK (transcript_hash ~ '^[0-9a-f]{64}$'),
    -- Where its recording is kept. NULL for a capture whose source keeps none, or whose connector
    -- names nowhere to keep it.
    storage_location_id UUID REFERENCES storage_locations (id),
    recording_path TEXT CHECK (char_length(recording_path) <= 1024),
    recording_content_type TEXT CHECK (char_length(recording_content_type) <= 255),
    -- The SHA-256 of the recording's bytes as they were downloaded, as lowercase hex. NULL for a
    -- recording kept before captures were hashed, which is never downloaded again to hash it.
    recording_hash TEXT CHECK (recording_hash ~ '^[0-9a-f]{64}$'),
    -- Where its own clock falls on the meeting's: a moment `t` of its transcript and recording is
    -- `t + offset_ms` on the meeting's clock.
    offset_ms BIGINT NOT NULL DEFAULT 0,
    -- Whose it is: the person whose connector read it, or who uploaded it.
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT meeting_captures_once_per_connector UNIQUE (connector_id, external_id),
    CONSTRAINT meeting_captures_recording_fields
        CHECK ((recording_path IS NULL) = (storage_location_id IS NULL)),
    CONSTRAINT meeting_captures_recording_hash
        CHECK (recording_hash IS NULL OR recording_path IS NOT NULL)
);

SELECT diesel_manage_updated_at('meeting_captures');

-- A meeting's captures, first to last.
CREATE INDEX meeting_captures_of_meeting ON meeting_captures (meeting_id, created_at, id);

-- Every change to a meeting, one row per change, shaped like `page_events`: building its final
-- transcript, an edit of its words, a mark added or removed, a hold, an approval, and the undo of
-- any of them. Its words name the change that wrote them and the one that replaced them.
CREATE TABLE meeting_events (
    -- UUIDv7, so entries order as they were written.
    id UUID PRIMARY KEY,
    meeting_id UUID NOT NULL REFERENCES meetings (id) ON DELETE CASCADE,
    -- Text, as for pages, so a later kind needs no migration.
    kind TEXT NOT NULL CHECK (kind ~ '^[a-z][a-z_]{0,31}$'),
    -- Who made the change: a person, or the machine user standing for one of Elysium's own.
    created_by UUID NOT NULL REFERENCES users (id),
    -- The agent acting for them, as on `page_events`.
    agent TEXT CHECK (char_length(agent) BETWEEN 1 AND 300),
    agent_name TEXT CHECK (char_length(agent_name) BETWEEN 1 AND 200),
    -- A JSON object shaped by `kind`, and `undid` for an undo.
    data JSONB NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(data) = 'object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One meeting's history in order.
CREATE INDEX meeting_events_of_meeting ON meeting_events (meeting_id, id);

-- The final transcript: every word it ever held, each with a stable id. A word an edit replaced
-- is kept, marked by the change that replaced it, so an undo puts it back and every mark and
-- history entry naming it still resolves. The transcript is its standing words in order.
CREATE TABLE meeting_words (
    id UUID PRIMARY KEY,
    meeting_id UUID NOT NULL REFERENCES meetings (id) ON DELETE CASCADE,
    -- Its place among every word the meeting ever held, standing or replaced: gapped, so an edit
    -- writes its new words between two neighbours without moving the rest. New words go just
    -- after the words they replace, so an undo puts the old ones back exactly where they were.
    position BIGINT NOT NULL,
    -- The word as written, punctuation and all, with no space around it.
    text TEXT NOT NULL CHECK (text <> ''),
    -- Who said it, as its capture names them; empty when nothing says.
    speaker TEXT NOT NULL DEFAULT '',
    -- On the meeting's clock, in milliseconds. A capture that began before the meeting's clock
    -- starts places its first words before zero.
    start_ms BIGINT NOT NULL,
    end_ms BIGINT NOT NULL,
    -- Where it came from: its capture's source transcript, its capture's Whisper transcript, a
    -- correction of what the capture heard, or an edit.
    origin TEXT NOT NULL CHECK (origin IN ('source', 'whisper', 'correction', 'edit')),
    -- The capture it was heard in; NULL for an edit's words, which no capture said.
    capture_id UUID REFERENCES meeting_captures (id) ON DELETE SET NULL,
    -- The change that wrote it, and the one that replaced it, NULL while it stands.
    introduced_by UUID NOT NULL REFERENCES meeting_events (id),
    superseded_by UUID REFERENCES meeting_events (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT meeting_words_span CHECK (end_ms >= start_ms),
    -- Immediate, but deferrable, so one statement can renumber a meeting's words when an edit
    -- finds no room between two neighbours.
    CONSTRAINT meeting_words_order UNIQUE (meeting_id, position) DEFERRABLE INITIALLY IMMEDIATE
);

-- The transcript as it reads now, in order.
CREATE INDEX meeting_words_standing ON meeting_words (meeting_id, position)
    WHERE superseded_by IS NULL;

-- Everything anchored to a range of the final transcript: confidence flags, edits, excerpts,
-- black-outs, comments, pins, and tasks. A range runs from one standing word to another, so it
-- can be one word or the whole meeting; an edit moves every mark over the words it replaces onto
-- the words that replace them.
CREATE TABLE meeting_marks (
    id UUID PRIMARY KEY,
    meeting_id UUID NOT NULL REFERENCES meetings (id) ON DELETE CASCADE,
    -- Text with its kinds in code, as history's are, so a later kind needs no migration.
    kind TEXT NOT NULL CHECK (kind ~ '^[a-z][a-z_]{0,31}$'),
    start_word_id UUID NOT NULL REFERENCES meeting_words (id),
    end_word_id UUID NOT NULL REFERENCES meeting_words (id),
    -- A JSON object shaped by `kind`, such as a confidence flag's confidence and alternatives.
    data JSONB NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(data) = 'object'),
    -- Who made it, and the agent acting for them, as on `meeting_events`.
    created_by UUID NOT NULL REFERENCES users (id),
    agent TEXT CHECK (char_length(agent) BETWEEN 1 AND 300),
    agent_name TEXT CHECK (char_length(agent_name) BETWEEN 1 AND 200),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- The change that removed it, NULL while it stands. Kept, so an undo puts it back with
    -- whatever hangs from it.
    removed_by UUID REFERENCES meeting_events (id)
);

CREATE INDEX meeting_marks_of_meeting ON meeting_marks (meeting_id, id);

-- Each meeting's one capture: its connector, its source's ids, and what its fetch stored.
INSERT INTO meeting_captures (
    id, meeting_id, connector_id, source_kind, external_id, url, transcript, transcript_hash,
    storage_location_id, recording_path, recording_content_type, created_by, created_at, updated_at
)
SELECT uuidv7(), id, connector_id, source_kind, external_id, url, transcript, transcript_hash,
    storage_location_id, recording_path, recording_content_type, created_by, created_at, updated_at
FROM meetings;

-- Whisper transcribes each capture on its own, so its transcript belongs to the capture it heard.
CREATE TABLE capture_transcriptions (
    capture_id UUID PRIMARY KEY REFERENCES meeting_captures (id) ON DELETE CASCADE,
    -- `crate::whisper::WhisperTranscript`: the model, the language, how long the audio runs,
    -- and its timed segments, each with its timed words, every time in milliseconds from the start
    -- of the capture's own recording.
    transcript JSONB NOT NULL CHECK (jsonb_typeof(transcript) = 'object'),
    -- The SHA-256 of `transcript` as it was serialized to be stored, as lowercase hex: what later
    -- steps name the transcription they read by.
    transcript_hash TEXT NOT NULL CHECK (transcript_hash ~ '^[0-9a-f]{64}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO capture_transcriptions (capture_id, transcript, transcript_hash, created_at)
SELECT captures.id, transcriptions.transcript, transcriptions.transcript_hash,
    transcriptions.created_at
FROM meeting_transcriptions transcriptions
JOIN meeting_captures captures ON captures.meeting_id = transcriptions.meeting_id;

DROP TABLE meeting_transcriptions;
ALTER TABLE capture_transcriptions RENAME TO meeting_transcriptions;
ALTER TABLE meeting_transcriptions RENAME CONSTRAINT capture_transcriptions_pkey
    TO meeting_transcriptions_pkey;
ALTER TABLE meeting_transcriptions RENAME CONSTRAINT capture_transcriptions_capture_id_fkey
    TO meeting_transcriptions_capture_id_fkey;
ALTER TABLE meeting_transcriptions RENAME CONSTRAINT capture_transcriptions_transcript_check
    TO meeting_transcriptions_transcript_check;
ALTER TABLE meeting_transcriptions RENAME CONSTRAINT capture_transcriptions_transcript_hash_check
    TO meeting_transcriptions_transcript_hash_check;

-- The lines each meeting's final transcript is built from, with where each came from, exactly as
-- `crate::meetings::final_transcript` builds them:
--
-- - a meeting that was corrected, from its corrected copy, a line differing from the one it
--   follows being the correction's;
-- - a meeting segmented without a correction, from what it was segmented from: its source's lines,
--   or Whisper's segments when the source sent none.
--
-- A meeting not yet corrected or segmented has its transcript built by its run, as a new one does.
CREATE TEMPORARY TABLE final_lines (
    meeting_id UUID NOT NULL,
    capture_id UUID NOT NULL,
    built_from TEXT NOT NULL,
    line_number BIGINT NOT NULL,
    start_ms BIGINT NOT NULL,
    end_ms BIGINT NOT NULL,
    speaker TEXT NOT NULL,
    text TEXT NOT NULL,
    origin TEXT NOT NULL
);

-- The lines a capture heard, as `crate::meetings::correction::base` reads them: the source's
-- lines, or Whisper's segments that say something, trimmed, when the source sent none.
CREATE TEMPORARY TABLE heard_lines AS
SELECT captures.id AS capture_id, 'source' AS heard_from, line.number AS line_number,
    (line.value->>'startMs')::BIGINT AS start_ms, (line.value->>'endMs')::BIGINT AS end_ms,
    COALESCE(line.value->>'speaker', '') AS speaker, COALESCE(line.value->>'text', '') AS text
FROM meeting_captures captures,
    jsonb_array_elements(captures.transcript) WITH ORDINALITY AS line (value, number)
UNION ALL
SELECT captures.id, 'whisper', row_number() OVER (
        PARTITION BY captures.id ORDER BY segment.number
    ),
    (segment.value->>'startMs')::BIGINT, (segment.value->>'endMs')::BIGINT, '',
    btrim(segment.value->>'text', E' \t\n\r')
FROM meeting_captures captures
JOIN meeting_transcriptions transcriptions ON transcriptions.capture_id = captures.id,
    jsonb_array_elements(transcriptions.transcript->'segments')
        WITH ORDINALITY AS segment (value, number)
WHERE jsonb_array_length(captures.transcript) = 0
    AND btrim(segment.value->>'text', E' \t\n\r') <> '';

INSERT INTO final_lines
SELECT corrections.meeting_id, captures.id, 'correction', line.number,
    (line.value->>'startMs')::BIGINT, (line.value->>'endMs')::BIGINT,
    COALESCE(line.value->>'speaker', ''), COALESCE(line.value->>'text', ''),
    CASE
        WHEN heard.text IS NOT DISTINCT FROM line.value->>'text' THEN corrections.based_on
        ELSE 'correction'
    END
FROM meeting_corrections corrections
JOIN meeting_captures captures ON captures.meeting_id = corrections.meeting_id
CROSS JOIN LATERAL
    jsonb_array_elements(corrections.transcript) WITH ORDINALITY AS line (value, number)
LEFT JOIN heard_lines heard ON heard.capture_id = captures.id
    AND heard.heard_from = corrections.based_on
    AND heard.line_number = line.number;

INSERT INTO final_lines
SELECT captures.meeting_id, captures.id, heard.heard_from, heard.line_number, heard.start_ms,
    heard.end_ms, heard.speaker, heard.text, heard.heard_from
FROM meeting_captures captures
JOIN meetings ON meetings.id = captures.meeting_id
JOIN heard_lines heard ON heard.capture_id = captures.id
WHERE meetings.state IN ('segmented', 'done')
    AND NOT EXISTS (
        SELECT 1 FROM meeting_corrections corrections
        WHERE corrections.meeting_id = captures.meeting_id
    );

-- Each line's words, split at whitespace, its time spread evenly across them.
CREATE TEMPORARY TABLE final_words AS
SELECT lines.meeting_id, lines.capture_id, lines.built_from, lines.line_number,
    word.number AS word_number, word.count AS word_count, word.text, lines.speaker,
    GREATEST(lines.start_ms, 0) AS start_ms,
    GREATEST(lines.end_ms, lines.start_ms, 0) AS end_ms, lines.origin
FROM final_lines lines,
    LATERAL (
        SELECT token AS text, row_number() OVER (ORDER BY token_number) AS number,
            count(*) OVER () AS count
        FROM regexp_split_to_table(lines.text, '\s+') WITH ORDINALITY AS split (token, token_number)
        WHERE token <> ''
    ) AS word;

-- One history entry per meeting built: the build is the change its words name.
CREATE TEMPORARY TABLE builds AS
SELECT meeting_id, capture_id, built_from, count(*) AS words, uuidv7() AS event_id
FROM final_words
GROUP BY meeting_id, capture_id, built_from;

INSERT INTO meeting_events (id, meeting_id, kind, created_by, data)
SELECT event_id, meeting_id, 'built', '00000000-0000-0000-0000-000000000001',
    jsonb_build_object('captureId', capture_id, 'builtFrom', built_from, 'words', words)
FROM builds;

INSERT INTO meeting_words (
    id, meeting_id, position, text, speaker, start_ms, end_ms, origin, capture_id, introduced_by
)
SELECT uuidv7(), words.meeting_id,
    1024 * row_number() OVER (
        PARTITION BY words.meeting_id ORDER BY words.line_number, words.word_number
    ),
    words.text, words.speaker,
    words.start_ms + (words.end_ms - words.start_ms) * (words.word_number - 1) / words.word_count,
    words.start_ms + (words.end_ms - words.start_ms) * words.word_number / words.word_count,
    words.origin, words.capture_id, builds.event_id
FROM final_words words
JOIN builds ON builds.meeting_id = words.meeting_id;

-- Dropped here rather than at commit: `diesel migration redo --all` runs every migration in one
-- transaction.
DROP TABLE builds, final_words, final_lines, heard_lines;

-- What moved onto each meeting's capture leaves the meeting, which keeps the event's own fields.
ALTER TABLE meetings
    DROP CONSTRAINT meetings_recording_fields,
    DROP COLUMN connector_id,
    DROP COLUMN source_kind,
    DROP COLUMN external_id,
    DROP COLUMN url,
    DROP COLUMN transcript,
    DROP COLUMN transcript_hash,
    DROP COLUMN storage_location_id,
    DROP COLUMN recording_path,
    DROP COLUMN recording_content_type;
