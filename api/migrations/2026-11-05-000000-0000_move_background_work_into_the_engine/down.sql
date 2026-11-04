ALTER TABLE database_backups DROP COLUMN claimed_at;

ALTER TABLE connectors DROP COLUMN syncing_since;

ALTER TABLE browser_sessions
    DROP CONSTRAINT browser_sessions_error_when_failed,
    DROP COLUMN state_error,
    DROP COLUMN state;
DROP TYPE browser_state;

DROP TABLE update_checks;
DROP TABLE process_activities;
DROP TABLE agent_tool_grants;

DROP TABLE session_turns;
DROP TYPE session_turn_status;
