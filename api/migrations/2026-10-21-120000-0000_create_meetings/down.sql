DROP TABLE meetings;

-- Accounts that sign in with a credential have no browser, and cannot exist without the
-- columns this migration added.
DELETE FROM connector_accounts WHERE browser_session_id IS NULL;

ALTER TABLE connector_accounts
    DROP CONSTRAINT connector_accounts_one_sign_in,
    DROP COLUMN storage_location_id,
    DROP COLUMN credential_encrypted,
    ALTER COLUMN browser_session_id SET NOT NULL;
