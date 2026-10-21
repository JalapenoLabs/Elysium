-- Restores the satellite schema. Rows dropped going up (satellites, transcripts, protobuf
-- events) are not brought back; sessions come back with no satellite and a placeholder thread.

CREATE TABLE satellites (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    url TEXT NOT NULL,
    secret_encrypted BYTEA NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID NOT NULL REFERENCES users (id),

    CONSTRAINT satellites_name_unique UNIQUE (name),
    CONSTRAINT satellites_name_length CHECK (char_length(name) BETWEEN 1 AND 120),
    CONSTRAINT satellites_description_length CHECK (char_length(description) <= 2000),
    CONSTRAINT satellites_url_scheme CHECK (url ~ '^https?://' AND char_length(url) <= 2048),
    CONSTRAINT satellites_secret_sealed CHECK (octet_length(secret_encrypted) >= 41)
);
SELECT diesel_manage_updated_at('satellites');

CREATE TABLE session_transcripts (
    session_id BIGINT PRIMARY KEY REFERENCES coding_sessions (id) ON DELETE CASCADE,
    harness TEXT NOT NULL,
    harness_session_id TEXT NOT NULL,
    transcript_sealed BYTEA NOT NULL,
    size_bytes BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT session_transcripts_harness_known CHECK (harness IN ('claude', 'codex')),
    CONSTRAINT session_transcripts_harness_session_id_length
        CHECK (char_length(harness_session_id) BETWEEN 1 AND 200),
    CONSTRAINT session_transcripts_size_unsigned CHECK (size_bytes >= 0)
);
SELECT diesel_manage_updated_at('session_transcripts');

ALTER TABLE coding_sessions DROP COLUMN harness_session_id;
CREATE TYPE session_continuation AS ENUM (
    'imported',
    'brief'
);
ALTER TABLE coding_sessions ADD COLUMN continuation session_continuation;
ALTER TABLE coding_sessions
    ADD CONSTRAINT coding_sessions_continuation_studio
    CHECK (continuation IS NULL OR studio_item_id IS NOT NULL);
ALTER TABLE coding_sessions ADD COLUMN satellite_id UUID
    REFERENCES satellites (id) ON DELETE SET NULL;
ALTER TABLE coding_sessions ADD COLUMN thread_id TEXT NOT NULL DEFAULT 'none';
ALTER TABLE coding_sessions ALTER COLUMN thread_id DROP DEFAULT;
ALTER TABLE coding_sessions
    ADD CONSTRAINT coding_sessions_thread_id_length CHECK (char_length(thread_id) BETWEEN 1 AND 200);
ALTER TABLE coding_sessions
    ADD CONSTRAINT coding_sessions_thread_unique UNIQUE (satellite_id, thread_id);

DELETE FROM session_events;
ALTER TABLE session_events ALTER COLUMN occurred_at DROP NOT NULL;
ALTER TABLE session_events DROP COLUMN payload;
ALTER TABLE session_events DROP COLUMN turn_id;
ALTER TABLE session_events ADD COLUMN event BYTEA NOT NULL;
