-- Meetings: recordings and transcripts read from a meeting source, cut into topic segments
-- that land on the timeline. See docs/meetings.md.

-- A connector account signs in one of two ways: through one of the person's browsers, or with
-- a credential they gave Elysium (an API key), sealed like every application secret. Meeting
-- sources such as Fireflies use the second, so every meeting still belongs to an account and
-- reading it twice records it once.
ALTER TABLE connector_accounts
    ALTER COLUMN browser_session_id DROP NOT NULL,
    -- An envelope from the cipher, bound to the row's id.
    ADD COLUMN credential_encrypted BYTEA,
    -- Where a connector that keeps files saves them, such as meeting recordings.
    ADD COLUMN storage_location_id UUID REFERENCES storage_locations (id),
    ADD CONSTRAINT connector_accounts_one_sign_in
        CHECK ((browser_session_id IS NULL) <> (credential_encrypted IS NULL));

CREATE TABLE meetings (
    id UUID PRIMARY KEY,
    -- The account it was read from. Kept when the account is removed, so the history stays.
    connector_account_id UUID REFERENCES connector_accounts (id) ON DELETE SET NULL,
    -- Which source it came from, such as `fireflies`.
    source_kind TEXT NOT NULL CHECK (source_kind ~ '^[a-z][a-z0-9_]{0,31}$'),
    -- The source's own id for it, unique per account.
    external_id TEXT NOT NULL CHECK (char_length(external_id) BETWEEN 1 AND 1024),
    title TEXT NOT NULL DEFAULT '' CHECK (char_length(title) <= 2048),
    started_at TIMESTAMPTZ NOT NULL,
    duration_ms BIGINT CHECK (duration_ms >= 0),
    -- Who organized it, as the source shows them.
    organizer TEXT NOT NULL DEFAULT '' CHECK (char_length(organizer) <= 512),
    -- Who took part: an array of strings, as the source shows them.
    participants JSONB NOT NULL DEFAULT '[]',
    -- The meeting's page at its source.
    url TEXT CHECK (char_length(url) <= 4096),
    -- The timestamped transcript: an array of `{ startMs, endMs, speaker, text }`.
    transcript JSONB NOT NULL DEFAULT '[]',
    -- Where the recording is kept. NULL until it is stored, and for a meeting the source
    -- keeps no recording of.
    storage_location_id UUID REFERENCES storage_locations (id),
    recording_path TEXT CHECK (char_length(recording_path) <= 1024),
    recording_content_type TEXT CHECK (char_length(recording_content_type) <= 255),
    -- The topic segments Elysia found: an array of `{ startMs, endMs, title, summary,
    -- projects: [{ projectId, confidence }] }`. NULL until she has read the transcript.
    segments JSONB,
    -- How far processing got: `pending` (read), `recorded` (the recording is stored, or there
    -- is none), `segmented`, `done` (its segments are on the timeline), or `failed`.
    state TEXT NOT NULL DEFAULT 'pending'
        CHECK (state IN ('pending', 'recorded', 'segmented', 'done', 'failed')),
    -- How many times its current step failed; it is `failed` once that reaches the limit.
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    -- Why the last attempt failed, or NULL when it succeeded.
    last_error TEXT,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (connector_account_id, external_id),
    CONSTRAINT meetings_recording_fields
        CHECK ((recording_path IS NULL) = (storage_location_id IS NULL))
);

SELECT diesel_manage_updated_at('meetings');

-- The processing loop picks up every meeting that is not finished.
CREATE INDEX meetings_unfinished ON meetings (created_at) WHERE state NOT IN ('done', 'failed');
