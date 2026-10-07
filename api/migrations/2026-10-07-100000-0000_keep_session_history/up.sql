-- Elysium keeps what a thread needs to carry on; satellites keep nothing for good. See
-- docs/coding.md: a session's events and its harness session outlive the thread, the
-- satellite, and the satellite's registration.

-- Deleting a satellite no longer forgets its sessions. They keep their history and can be
-- continued on another satellite; only the pointer to the deleted one goes.
ALTER TABLE coding_sessions DROP CONSTRAINT coding_sessions_satellite_id_fkey;
ALTER TABLE coding_sessions ALTER COLUMN satellite_id DROP NOT NULL;
ALTER TABLE coding_sessions
    ADD CONSTRAINT coding_sessions_satellite_id_fkey
    FOREIGN KEY (satellite_id) REFERENCES satellites (id) ON DELETE SET NULL;

-- Every event a session's thread emitted, as the satellite sent it: the protobuf
-- `ThreadEvent`, so a later build renders old events with whatever it knows then.
CREATE TABLE session_events (
    session_id BIGINT NOT NULL REFERENCES coding_sessions (id) ON DELETE CASCADE,
    -- The satellite's sequence, strictly increasing per thread. Unsigned on the wire.
    sequence BIGINT NOT NULL,
    occurred_at TIMESTAMPTZ,
    event BYTEA NOT NULL,

    PRIMARY KEY (session_id, sequence),
    CONSTRAINT session_events_sequence_unsigned CHECK (sequence >= 0)
);

-- The harness's own session (Claude's transcript, Codex's rollout), as the satellite
-- exported it after the thread's latest turn. It holds everything the agent read, which
-- the satellite does not scrub, so it is compressed and then sealed like any secret.
CREATE TABLE session_transcripts (
    session_id BIGINT PRIMARY KEY REFERENCES coding_sessions (id) ON DELETE CASCADE,
    -- The harness family that wrote it; a thread of the other family cannot resume it.
    harness TEXT NOT NULL,
    -- The harness's id for the session, which a resumed thread continues.
    harness_session_id TEXT NOT NULL,
    transcript_sealed BYTEA NOT NULL,
    -- The export's size before compression.
    size_bytes BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT session_transcripts_harness_known CHECK (harness IN ('claude', 'codex')),
    CONSTRAINT session_transcripts_harness_session_id_length
        CHECK (char_length(harness_session_id) BETWEEN 1 AND 200),
    CONSTRAINT session_transcripts_size_unsigned CHECK (size_bytes >= 0)
);

SELECT diesel_manage_updated_at('session_transcripts');
