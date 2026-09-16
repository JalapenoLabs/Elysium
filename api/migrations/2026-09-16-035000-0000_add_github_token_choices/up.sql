-- Which GitHub token a coding session's agent works with, chosen at three levels: a
-- default for the whole workspace, a project's choice, and a session's choice when it
-- starts. See docs/github.md.

-- The workspace default. At most one token is the default; none means sessions get no
-- token unless their project or the session names one.
ALTER TABLE github_credentials
    ADD COLUMN is_default BOOLEAN NOT NULL DEFAULT false;

CREATE UNIQUE INDEX github_credentials_one_default
    ON github_credentials (is_default)
    WHERE is_default;

-- How a project picks its token. 'default' follows the workspace default, 'none' never
-- gives its sessions a token, and 'specific' names one.
CREATE TYPE github_access AS ENUM (
    'default',
    'none',
    'specific'
);

-- Deleting a token leaves a 'specific' project with no token id. That project then
-- follows the workspace default, and says so, rather than refusing the delete: a
-- revoked token must always be removable.
ALTER TABLE projects
    ADD COLUMN github_access github_access NOT NULL DEFAULT 'default',
    ADD COLUMN github_credential_id UUID REFERENCES github_credentials (id) ON DELETE SET NULL,
    ADD CONSTRAINT projects_github_credential_is_specific CHECK (
        github_access = 'specific' OR github_credential_id IS NULL
    );

-- The token a session's thread was started with. A thread takes its environment once,
-- at creation, so this is a record of what the agent holds, not a live choice.
ALTER TABLE coding_sessions
    ADD COLUMN github_credential_id UUID REFERENCES github_credentials (id) ON DELETE SET NULL;
