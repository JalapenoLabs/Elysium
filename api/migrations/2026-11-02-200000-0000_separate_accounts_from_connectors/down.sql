-- Undoes up.sql: connectors become connector accounts again, each holding its own key.
--
-- A key goes back to every connector that read through its account. It was sealed bound to the
-- account's id, so it opens again only on the connector that shares that id, which every
-- account carried over by up.sql does. An account made since then gets its key back on a row
-- it does not open on, and has to be connected again.

DROP TABLE connector_runs;
DROP TABLE connector_check_results;

ALTER TABLE timeline_records RENAME COLUMN connector_id TO connector_account_id;
ALTER TABLE meetings RENAME COLUMN connector_id TO connector_account_id;

DELETE FROM connectors WHERE jira_credential_id IS NOT NULL OR github_credential_id IS NOT NULL;

DROP INDEX connectors_github_credential_kind;
DROP INDEX connectors_jira_credential_kind;
DROP INDEX connectors_token_account_kind;
DROP INDEX connectors_browser_session_kind;

ALTER TABLE connectors
    DROP CONSTRAINT connectors_one_account,
    ADD COLUMN credential_encrypted BYTEA;

UPDATE connectors
SET credential_encrypted = token_accounts.credential_encrypted
FROM token_accounts
WHERE connectors.token_account_id = token_accounts.id;

ALTER TABLE connectors
    DROP COLUMN content,
    DROP COLUMN github_credential_id,
    DROP COLUMN jira_credential_id,
    DROP COLUMN token_account_id,
    ADD CONSTRAINT connector_accounts_browser_session_id_key UNIQUE (browser_session_id),
    ADD CONSTRAINT connector_accounts_one_sign_in
        CHECK ((browser_session_id IS NULL) <> (credential_encrypted IS NULL));

ALTER INDEX connectors_pkey RENAME TO connector_accounts_pkey;
ALTER TABLE connectors RENAME TO connector_accounts;

DROP TABLE token_accounts;
