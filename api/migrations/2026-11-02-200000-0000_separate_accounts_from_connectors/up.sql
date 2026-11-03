-- Accounts and connectors become separate. An account is how Elysium signs in to a provider as
-- a person; a connector is one use of an account, which says what it reads and holds what its
-- reading has found. One account can feed several connectors. See docs/connectors.md.
--
-- Accounts keep the stores they already have, one per way of signing in: Atlassian accounts in
-- jira_credentials, GitHub accounts in github_credentials, signed-in browsers in
-- browser_sessions. The new token_accounts holds every account whose sign-in is one API key.

CREATE TABLE token_accounts (
    id UUID PRIMARY KEY,
    -- The provider the key is for, such as `fireflies`.
    kind TEXT NOT NULL CHECK (kind ~ '^[a-z][a-z0-9_]{0,31}$'),
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    -- An envelope from the cipher, bound to the row's id.
    credential_encrypted BYTEA NOT NULL,
    -- Who the provider says the key belongs to, as of the last check, such as an address.
    identity TEXT NOT NULL DEFAULT '' CHECK (char_length(identity) <= 512),
    checked_at TIMESTAMPTZ,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

SELECT diesel_manage_updated_at('token_accounts');

-- Every key a connector held becomes an account of its own. It keeps the connector's id, the
-- id its envelope is bound to, so the sealed key still opens.
INSERT INTO token_accounts (id, kind, name, credential_encrypted, created_by, created_at, updated_at)
SELECT id, kind, initcap(kind), credential_encrypted, created_by, created_at, updated_at
FROM connector_accounts
WHERE credential_encrypted IS NOT NULL;

ALTER TABLE connector_accounts RENAME TO connectors;
ALTER INDEX connector_accounts_pkey RENAME TO connectors_pkey;

ALTER TABLE connectors
    DROP CONSTRAINT connector_accounts_one_sign_in,
    DROP CONSTRAINT connector_accounts_browser_session_id_key,
    -- The account it reads through: exactly one of the four, which its kind decides.
    ADD COLUMN token_account_id UUID REFERENCES token_accounts (id) ON DELETE CASCADE,
    ADD COLUMN jira_credential_id UUID REFERENCES jira_credentials (id) ON DELETE CASCADE,
    ADD COLUMN github_credential_id UUID REFERENCES github_credentials (id) ON DELETE CASCADE,
    -- What it reads, in its kind's own terms, for a kind whose selection has no table of its own.
    ADD COLUMN content JSONB NOT NULL DEFAULT '{}';

UPDATE connectors SET token_account_id = id WHERE credential_encrypted IS NOT NULL;

ALTER TABLE connectors
    DROP COLUMN credential_encrypted,
    ADD CONSTRAINT connectors_one_account CHECK (
        num_nonnulls(browser_session_id, token_account_id, jira_credential_id, github_credential_id) = 1
    );

-- An account feeds each kind at most once: a Microsoft browser can feed Outlook and Teams, but
-- not Outlook twice.
CREATE UNIQUE INDEX connectors_browser_session_kind ON connectors (browser_session_id, kind);
CREATE UNIQUE INDEX connectors_token_account_kind ON connectors (token_account_id, kind);
CREATE UNIQUE INDEX connectors_jira_credential_kind ON connectors (jira_credential_id, kind);
CREATE UNIQUE INDEX connectors_github_credential_kind ON connectors (github_credential_id, kind);

-- Every Atlassian and GitHub account carries its connector: links can be made through any of
-- them and the link watcher reads through each, so the connector is where that is shown,
-- checked, and paused.
INSERT INTO connectors (id, kind, jira_credential_id, created_by, created_at)
SELECT uuidv7(), 'jira', id, created_by, created_at FROM jira_credentials;

INSERT INTO connectors (id, kind, github_credential_id, created_by, created_at)
SELECT uuidv7(), 'github', id, created_by, created_at FROM github_credentials;

ALTER TABLE meetings RENAME COLUMN connector_account_id TO connector_id;
ALTER TABLE timeline_records RENAME COLUMN connector_account_id TO connector_id;

-- The last result of each of a connector's checks, so the hub shows them after a reload.
CREATE TABLE connector_check_results (
    connector_id UUID NOT NULL REFERENCES connectors (id) ON DELETE CASCADE,
    -- The check, as its kind names it, such as `sign_in`.
    check_id TEXT NOT NULL CHECK (check_id ~ '^[a-z][a-z0-9_]{0,63}$'),
    state TEXT NOT NULL CHECK (state IN ('passed', 'failed', 'skipped')),
    -- How many it found, for a check that counts, such as projects reached.
    count BIGINT CHECK (count >= 0),
    -- Why, in Elysium's own terms, such as `no_messages`; the web app words it.
    reason TEXT CHECK (reason ~ '^[a-z][a-z0-9_]{0,63}$'),
    -- What the provider said, for a check that failed on its answer.
    message TEXT CHECK (char_length(message) <= 2048),
    -- The check it waited on, for a check skipped because that one did not pass.
    blocked_by TEXT CHECK (blocked_by ~ '^[a-z][a-z0-9_]{0,63}$'),
    ran_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (connector_id, check_id)
);

-- Syncs worth showing: the ones that recorded something, and the ones that failed. Each
-- connector keeps its newest few; see api/src/models/connector_run.rs.
CREATE TABLE connector_runs (
    id UUID PRIMARY KEY,
    connector_id UUID NOT NULL REFERENCES connectors (id) ON DELETE CASCADE,
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('succeeded', 'failed')),
    -- How many things it recorded: timeline records, or changes to linked issues.
    records INTEGER NOT NULL DEFAULT 0 CHECK (records >= 0),
    error TEXT CHECK (char_length(error) <= 2048)
);

CREATE INDEX connector_runs_connector ON connector_runs (connector_id, started_at DESC);
CREATE INDEX connector_runs_started_at ON connector_runs (started_at DESC);
