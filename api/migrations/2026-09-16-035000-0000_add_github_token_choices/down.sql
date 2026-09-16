ALTER TABLE coding_sessions
    DROP COLUMN github_credential_id;

ALTER TABLE projects
    DROP CONSTRAINT projects_github_credential_is_specific,
    DROP COLUMN github_credential_id,
    DROP COLUMN github_access;

DROP TYPE github_access;

DROP INDEX github_credentials_one_default;

ALTER TABLE github_credentials
    DROP COLUMN is_default;
