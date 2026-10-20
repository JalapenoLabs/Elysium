-- The git repository every agent's `~/.claude` is checked out from: instructions, skills, and
-- settings. NULL leaves the workspace's own directory as it is. An https URL without
-- credentials; a github.com repository is read with the workspace's default GitHub token.
ALTER TABLE workspace_settings
    ADD COLUMN agent_configuration_repository TEXT
        CONSTRAINT workspace_settings_agent_configuration_repository_shape
        CHECK (
            agent_configuration_repository ~ '^https://[^/@\s]+/\S+$'
            AND length(agent_configuration_repository) <= 2048
        );
