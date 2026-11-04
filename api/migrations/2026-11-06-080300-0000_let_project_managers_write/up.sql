-- What a project's Project Manager writes to its trackers, and where. See docs/engine.md,
-- Project Managers.

-- Whether the Project Manager's changes apply as it makes them. Off, the default, it proposes
-- each as a changeset a person approves first: the safer way for a project to start.
ALTER TABLE projects
    ADD COLUMN manager_writes_directly BOOLEAN NOT NULL DEFAULT FALSE;

-- The tracker a project names for the issues its Project Manager files, overriding the
-- precedent of the issues its items already link. A GitHub tracker names no credential: it
-- files with the token the project's sessions get, its own or the workspace default.
CREATE TABLE project_issue_trackers (
    project_id UUID PRIMARY KEY REFERENCES projects (id) ON DELETE CASCADE,
    provider link_provider NOT NULL,
    jira_credential_id UUID REFERENCES jira_credentials (id) ON DELETE CASCADE,
    token_account_id UUID REFERENCES token_accounts (id) ON DELETE CASCADE,
    -- A Jira project key, a GitHub owner/name, or a YouTrack project's short name.
    destination TEXT NOT NULL,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT project_issue_trackers_credential CHECK (
        (provider::text = 'jira' AND jira_credential_id IS NOT NULL AND token_account_id IS NULL)
        OR (provider::text = 'github' AND jira_credential_id IS NULL AND token_account_id IS NULL)
        OR (provider::text = 'youtrack' AND token_account_id IS NOT NULL AND jira_credential_id IS NULL)
    ),
    CONSTRAINT project_issue_trackers_destination_length
        CHECK (char_length(destination) BETWEEN 1 AND 300)
);

SELECT diesel_manage_updated_at('project_issue_trackers');

-- The segment or message that caused a changeset operation, for the timeline record of the
-- write it makes. Every Project Manager's operation names one.
ALTER TABLE changeset_operations
    ADD COLUMN cause_record_id UUID REFERENCES timeline_records (id) ON DELETE SET NULL;
