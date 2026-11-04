-- A YouTrack link reaches its site through a YouTrack token account, the way a Jira link reaches
-- its site through a Jira credential. Deleting the account deletes the links that went through
-- it, and the watcher's cursor for it. Initiatives contain no YouTrack issues yet, so
-- initiative_links keeps refusing the provider through its existing checks.

ALTER TABLE action_item_links
    ADD COLUMN token_account_id UUID REFERENCES token_accounts (id) ON DELETE CASCADE;

ALTER TABLE action_item_links DROP CONSTRAINT action_item_links_credential;
ALTER TABLE action_item_links ADD CONSTRAINT action_item_links_credential CHECK (
    (provider::text = 'jira' AND jira_credential_id IS NOT NULL
        AND github_credential_id IS NULL AND token_account_id IS NULL)
    OR (provider::text = 'github' AND github_credential_id IS NOT NULL
        AND jira_credential_id IS NULL AND token_account_id IS NULL)
    OR (provider::text = 'youtrack' AND token_account_id IS NOT NULL
        AND jira_credential_id IS NULL AND github_credential_id IS NULL)
);

-- One YouTrack issue is one item, as for the other providers.
CREATE UNIQUE INDEX action_item_links_youtrack_unique
    ON action_item_links (token_account_id, kind, external_id)
    WHERE token_account_id IS NOT NULL;

ALTER TABLE link_watch_cursors
    ADD COLUMN token_account_id UUID UNIQUE REFERENCES token_accounts (id) ON DELETE CASCADE;

ALTER TABLE link_watch_cursors DROP CONSTRAINT link_watch_cursors_one_credential;
ALTER TABLE link_watch_cursors ADD CONSTRAINT link_watch_cursors_one_credential
    CHECK (num_nonnulls(jira_credential_id, github_credential_id, token_account_id) = 1);
