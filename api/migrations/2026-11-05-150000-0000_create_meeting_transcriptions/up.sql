-- What Elysium's own Whisper heard in a meeting's recording, word by word: one transcription
-- per meeting, written once by the meeting workflow's transcription step. It sits beside the
-- source's transcript (`meetings.transcript`) in a table of its own, because timing every word
-- makes it several times the source's size, and the meetings loop reads every unfinished
-- meeting's row each minute. A child of its meeting, so it carries no `created_by` of its own.
-- See docs/meetings.md, Transcription.
CREATE TABLE meeting_transcriptions (
    meeting_id UUID PRIMARY KEY REFERENCES meetings (id) ON DELETE CASCADE,
    -- `crate::whisper::WhisperTranscript`: the model, the language, how long the audio runs,
    -- and its timed segments, each with its timed words, every time in milliseconds.
    transcript JSONB NOT NULL CHECK (jsonb_typeof(transcript) = 'object'),
    -- The SHA-256 of `transcript` as it was serialized to be stored, as lowercase hex, like
    -- `meetings.transcript_hash`: what later steps name the transcription they read by.
    transcript_hash TEXT NOT NULL CHECK (transcript_hash ~ '^[0-9a-f]{64}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
