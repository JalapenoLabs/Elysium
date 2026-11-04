-- A meeting's corrected transcript: the transcript it follows, line by line, with misheard names
-- and terms fixed against the company doc, the meeting's other transcript, and other meetings,
-- written once by the meeting workflow's correction step. The raw transcripts are never changed;
-- this is a copy beside them, which every later step reads. A child of its meeting, so it
-- carries no `created_by` of its own. See docs/meetings.md, Correction.
CREATE TABLE meeting_corrections (
    meeting_id UUID PRIMARY KEY REFERENCES meetings (id) ON DELETE CASCADE,
    -- An array of `{ startMs, endMs, speaker, text }`, one per line of the transcript it
    -- follows, in its order and with its speakers and times: only the text differs.
    transcript JSONB NOT NULL CHECK (jsonb_typeof(transcript) = 'array'),
    -- The SHA-256 of `transcript` as it was serialized to be stored, as lowercase hex: what the
    -- step after names the copy it reads by.
    transcript_hash TEXT NOT NULL CHECK (transcript_hash ~ '^[0-9a-f]{64}$'),
    -- Which transcript it follows line by line: the source's, or Whisper's when the source sent
    -- no lines.
    based_on TEXT NOT NULL CHECK (based_on IN ('source', 'whisper')),
    -- The transcripts it read, by their hashes.
    source_transcript_hash TEXT NOT NULL CHECK (source_transcript_hash ~ '^[0-9a-f]{64}$'),
    whisper_transcript_hash TEXT CHECK (whisper_transcript_hash ~ '^[0-9a-f]{64}$'),
    -- How many lines it changed.
    changed_lines INTEGER NOT NULL CHECK (changed_lines >= 0),
    -- Every distinct fix, `{ heard, written }`, which later corrections of other meetings read.
    fixes JSONB NOT NULL CHECK (jsonb_typeof(fixes) = 'array'),
    -- Who approved it, and when; approval is optional.
    approved_by UUID REFERENCES users (id),
    approved_at TIMESTAMPTZ,
    -- The inbox item raised to review it when its run was held for approval.
    review_item_id UUID REFERENCES action_items (id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT meeting_corrections_approval CHECK ((approved_by IS NULL) = (approved_at IS NULL))
);

-- The item page finds the meeting an inbox item asks to review.
CREATE INDEX meeting_corrections_review_item ON meeting_corrections (review_item_id)
    WHERE review_item_id IS NOT NULL;

-- A meeting its person asked to approve before it is acted on, whatever its source's setting.
ALTER TABLE meetings ADD COLUMN hold_for_approval BOOLEAN NOT NULL DEFAULT false;
