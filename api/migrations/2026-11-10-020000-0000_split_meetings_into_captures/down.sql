-- Puts each meeting's capture back on the meeting, and its Whisper transcript back under the
-- meeting. The final transcript, its marks, and the meeting's history go: each was built from what
-- the meeting keeps (its corrected copy, or the transcript it was segmented from), and the code
-- of before reads none of them. A meeting holds one capture until captures can be merged, so the
-- first is the one there is.
ALTER TABLE meetings
    ADD COLUMN connector_id UUID REFERENCES connectors (id) ON DELETE SET NULL,
    ADD COLUMN source_kind TEXT CHECK (source_kind ~ '^[a-z][a-z0-9_]{0,31}$'),
    ADD COLUMN external_id TEXT CHECK (char_length(external_id) BETWEEN 1 AND 1024),
    ADD COLUMN url TEXT CHECK (char_length(url) <= 4096),
    ADD COLUMN transcript JSONB NOT NULL DEFAULT '[]',
    ADD COLUMN storage_location_id UUID REFERENCES storage_locations (id),
    ADD COLUMN recording_path TEXT CHECK (char_length(recording_path) <= 1024),
    ADD COLUMN recording_content_type TEXT CHECK (char_length(recording_content_type) <= 255),
    ADD COLUMN transcript_hash TEXT CHECK (transcript_hash ~ '^[0-9a-f]{64}$');

-- Its `updated_at` is the meeting's own, kept as it was.
ALTER TABLE meetings DISABLE TRIGGER set_updated_at;

UPDATE meetings
SET connector_id = captures.connector_id,
    source_kind = captures.source_kind,
    external_id = captures.external_id,
    url = captures.url,
    transcript = captures.transcript,
    storage_location_id = captures.storage_location_id,
    recording_path = captures.recording_path,
    recording_content_type = captures.recording_content_type,
    transcript_hash = captures.transcript_hash
FROM (
    SELECT DISTINCT ON (meeting_id) *
    FROM meeting_captures
    ORDER BY meeting_id, created_at, id
) AS captures
WHERE captures.meeting_id = meetings.id;

ALTER TABLE meetings ENABLE TRIGGER set_updated_at;

ALTER TABLE meetings
    ALTER COLUMN source_kind SET NOT NULL,
    ALTER COLUMN external_id SET NOT NULL,
    ADD CONSTRAINT meetings_connector_account_id_external_id_key UNIQUE (connector_id, external_id),
    ADD CONSTRAINT meetings_recording_fields
        CHECK ((recording_path IS NULL) = (storage_location_id IS NULL));

CREATE TABLE meeting_transcriptions_by_meeting (
    meeting_id UUID PRIMARY KEY REFERENCES meetings (id) ON DELETE CASCADE,
    transcript JSONB NOT NULL CHECK (jsonb_typeof(transcript) = 'object'),
    transcript_hash TEXT NOT NULL CHECK (transcript_hash ~ '^[0-9a-f]{64}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO meeting_transcriptions_by_meeting (meeting_id, transcript, transcript_hash, created_at)
SELECT DISTINCT ON (captures.meeting_id) captures.meeting_id, transcriptions.transcript,
    transcriptions.transcript_hash, transcriptions.created_at
FROM meeting_transcriptions transcriptions
JOIN meeting_captures captures ON captures.id = transcriptions.capture_id
ORDER BY captures.meeting_id, captures.created_at, captures.id;

DROP TABLE meeting_transcriptions;
ALTER TABLE meeting_transcriptions_by_meeting RENAME TO meeting_transcriptions;
ALTER TABLE meeting_transcriptions RENAME CONSTRAINT meeting_transcriptions_by_meeting_pkey
    TO meeting_transcriptions_pkey;
ALTER TABLE meeting_transcriptions
    RENAME CONSTRAINT meeting_transcriptions_by_meeting_meeting_id_fkey
    TO meeting_transcriptions_meeting_id_fkey;
ALTER TABLE meeting_transcriptions
    RENAME CONSTRAINT meeting_transcriptions_by_meeting_transcript_check
    TO meeting_transcriptions_transcript_check;
ALTER TABLE meeting_transcriptions
    RENAME CONSTRAINT meeting_transcriptions_by_meeting_transcript_hash_check
    TO meeting_transcriptions_transcript_hash_check;

DROP TABLE meeting_marks;
DROP TABLE meeting_words;
DROP TABLE meeting_events;
DROP TABLE meeting_captures;
