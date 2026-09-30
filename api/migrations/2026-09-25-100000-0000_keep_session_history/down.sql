DROP TABLE session_transcripts;
DROP TABLE session_events;

-- A session without a satellite has nothing to return to under the old rule, which forgot
-- a satellite's sessions with it.
DELETE FROM coding_sessions WHERE satellite_id IS NULL;
ALTER TABLE coding_sessions DROP CONSTRAINT coding_sessions_satellite_id_fkey;
ALTER TABLE coding_sessions ALTER COLUMN satellite_id SET NOT NULL;
ALTER TABLE coding_sessions
    ADD CONSTRAINT coding_sessions_satellite_id_fkey
    FOREIGN KEY (satellite_id) REFERENCES satellites (id) ON DELETE CASCADE;
