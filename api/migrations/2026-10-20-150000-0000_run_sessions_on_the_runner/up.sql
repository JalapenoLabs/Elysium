-- Sessions run on Elysium's own runner, in its workspace container, instead of on Arsox
-- satellites. See docs/coding.md, The runner.

-- A session's events are the runner's, kept as Elysium's own JSON. Events kept until now are
-- satellite protobuf, which nothing here decodes any more, so they go.
DELETE FROM session_events;
ALTER TABLE session_events DROP COLUMN event;
ALTER TABLE session_events ADD COLUMN turn_id UUID;
ALTER TABLE session_events ADD COLUMN payload JSONB NOT NULL;
ALTER TABLE session_events ALTER COLUMN occurred_at SET NOT NULL;

-- A session is a directory in the workspace and the Claude session its turns resume, not a
-- thread on a satellite. The harness session id is Claude Code's own; NULL until the first turn
-- reports it.
ALTER TABLE coding_sessions DROP CONSTRAINT coding_sessions_thread_unique;
ALTER TABLE coding_sessions DROP COLUMN thread_id;
ALTER TABLE coding_sessions DROP COLUMN satellite_id;
ALTER TABLE coding_sessions DROP CONSTRAINT coding_sessions_continuation_studio;
ALTER TABLE coding_sessions DROP COLUMN continuation;
DROP TYPE session_continuation;
ALTER TABLE coding_sessions ADD COLUMN harness_session_id TEXT
    CONSTRAINT coding_sessions_harness_session_id_length
    CHECK (char_length(harness_session_id) BETWEEN 1 AND 200);

-- The workspace keeps Claude's transcripts itself, so nothing is exported to continue on.
DROP TABLE session_transcripts;

DROP TABLE satellites;
