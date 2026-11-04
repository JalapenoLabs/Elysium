-- YouTrack links and cursors have nowhere to go without their column, so they are removed with
-- it, as deleting their token account would remove them.

DELETE FROM link_watch_cursors WHERE token_account_id IS NOT NULL;
ALTER TABLE link_watch_cursors DROP CONSTRAINT link_watch_cursors_one_credential;
ALTER TABLE link_watch_cursors DROP COLUMN token_account_id;
ALTER TABLE link_watch_cursors ADD CONSTRAINT link_watch_cursors_one_credential
    CHECK (num_nonnulls(jira_credential_id, github_credential_id) = 1);

DELETE FROM action_item_links WHERE provider::text = 'youtrack';
DROP INDEX action_item_links_youtrack_unique;
ALTER TABLE action_item_links DROP CONSTRAINT action_item_links_credential;
ALTER TABLE action_item_links DROP COLUMN token_account_id;
ALTER TABLE action_item_links ADD CONSTRAINT action_item_links_credential CHECK (
    (provider::text = 'jira' AND jira_credential_id IS NOT NULL AND github_credential_id IS NULL)
    OR (provider::text = 'github' AND github_credential_id IS NOT NULL AND jira_credential_id IS NULL)
);
