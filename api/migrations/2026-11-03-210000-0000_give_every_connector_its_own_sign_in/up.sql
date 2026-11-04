-- Every connector owns its sign-in: an Atlassian or GitHub token, an API key, or a browser.
-- No two connectors share one, and removing a connector removes its sign-in. See
-- docs/connectors.md.
--
-- Shared sign-ins are split here. A Confluence connector reading through a Jira connector's
-- Atlassian account gets an account of its own, holding a copy of the same token. A browser
-- several connectors read through stays with the one it was started for, and the others wait
-- for a browser of their own, which their person signs in to from the connector's page.

-- A secret a migration copied into a new row. It is still sealed for the row it was copied
-- from, because SQL cannot seal: the API re-seals each one for its own row when it starts,
-- before anything reads it, and removes it from here (crate::models::secret_copy).
CREATE TABLE secret_copies (
    -- Where the copy lives, as `<table>.<secret>`.
    target TEXT NOT NULL CHECK (target IN ('jira_credentials.token')),
    row_id UUID NOT NULL,
    -- The row the copied envelope is still bound to.
    sealed_for UUID NOT NULL,
    PRIMARY KEY (target, row_id)
);

-- Each connector beside its Atlassian account's Jira connector gets an account of its own, a
-- copy of the shared one. The copy allows no Jira project or board: its token is Confluence's,
-- and Jira's allowlist stays with the Jira connector's account.
DO $$
DECLARE
    shared RECORD;
    copy_id UUID;
    copy_name TEXT;
    attempt INTEGER;
BEGIN
    FOR shared IN
        SELECT connectors.id AS connector_id,
               connectors.kind,
               connectors.created_by AS connector_creator,
               jira_credentials.*
        FROM connectors
        JOIN jira_credentials ON jira_credentials.id = connectors.jira_credential_id
        WHERE connectors.kind <> 'jira'
          AND EXISTS (
              SELECT 1 FROM connectors AS jira
              WHERE jira.jira_credential_id = connectors.jira_credential_id
                AND jira.kind = 'jira'
          )
        ORDER BY connectors.created_at, connectors.id
    LOOP
        -- Account names are unique; the copy is named for its kind, numbered on a clash.
        copy_name := left(shared.name, 105) || ' (' || initcap(shared.kind) || ')';
        attempt := 1;
        WHILE EXISTS (SELECT 1 FROM jira_credentials WHERE name = copy_name) LOOP
            attempt := attempt + 1;
            copy_name := left(shared.name, 100) || ' (' || initcap(shared.kind) || ' ' || attempt || ')';
        END LOOP;

        copy_id := uuidv7();
        INSERT INTO jira_credentials (
            id, name, site_url, account_email, token_encrypted, account_id, display_name,
            all_projects, all_boards, checked_at, created_by
        )
        VALUES (
            copy_id, copy_name, shared.site_url, shared.account_email, shared.token_encrypted,
            shared.account_id, shared.display_name, FALSE, FALSE, shared.checked_at,
            shared.connector_creator
        );
        INSERT INTO secret_copies (target, row_id, sealed_for)
        VALUES ('jira_credentials.token', copy_id, shared.id);
        UPDATE connectors SET jira_credential_id = copy_id WHERE id = shared.connector_id;
    END LOOP;
END
$$;

-- A connector has exactly one sign-in. The one exception is a connector of a kind read through
-- a browser that shared its browser with another below: it has none while it waits for a browser
-- of its own. A new connector always starts with its browser, so only these kinds can wait.
ALTER TABLE connectors
    DROP CONSTRAINT connectors_one_account,
    ADD CONSTRAINT connectors_one_sign_in CHECK (
        num_nonnulls(browser_session_id, token_account_id, jira_credential_id, github_credential_id) = 1
        OR (
            num_nonnulls(browser_session_id, token_account_id, jira_credential_id, github_credential_id) = 0
            AND kind IN ('outlook', 'gmail', 'teams', 'slack')
        )
    );

-- The kind of connector each browser's start page reads, by its host, mirroring the readers in
-- api/src/connectors/webmail.rs, teams.rs, and slack.rs. NULL for any other page.
CREATE TEMPORARY TABLE browser_kinds AS
SELECT id,
       CASE substring(start_url FROM '^https://([^/:?#]+)')
           WHEN 'outlook.office.com' THEN 'outlook'
           WHEN 'outlook.office365.com' THEN 'outlook'
           WHEN 'outlook.live.com' THEN 'outlook'
           WHEN 'mail.google.com' THEN 'gmail'
           WHEN 'teams.cloud.microsoft' THEN 'teams'
           WHEN 'teams.microsoft.com' THEN 'teams'
           WHEN 'teams.live.com' THEN 'teams'
           WHEN 'app.slack.com' THEN 'slack'
       END AS kind
FROM browser_sessions;

-- A browser several connectors read through stays with the one its start page is for, or else
-- the oldest. The others wait for a browser of their own: a browser's profile cannot be copied,
-- so their person signs in again, and until then each reads as signed out. Their cursors stay,
-- so nothing they already recorded is recorded again.
WITH ranked AS (
    SELECT connectors.id,
           row_number() OVER (
               PARTITION BY connectors.browser_session_id
               ORDER BY COALESCE(connectors.kind = browser_kinds.kind, FALSE) DESC,
                        connectors.created_at,
                        connectors.id
           ) AS position
    FROM connectors
    JOIN browser_kinds ON browser_kinds.id = connectors.browser_session_id
)
UPDATE connectors
SET browser_session_id = NULL, sign_in_state = 'signed_out'
FROM ranked
WHERE ranked.id = connectors.id AND ranked.position > 1;

-- A browser no connector reads becomes the connector its start page is for, paused, so the
-- sign-in its person made is kept and nothing is read until they resume it. A browser on any
-- other page has no connector to belong to and is removed; the browser reconciler then removes
-- its container, network, and profile, as for any browser without a row.
INSERT INTO connectors (id, kind, browser_session_id, is_active, content, created_by, created_at)
SELECT uuidv7(),
       browser_kinds.kind,
       browser_sessions.id,
       FALSE,
       CASE
           WHEN browser_kinds.kind IN ('teams', 'slack') THEN '{"conversations": "*"}'::jsonb
           ELSE '{}'::jsonb
       END,
       browser_sessions.created_by,
       browser_sessions.created_at
FROM browser_sessions
JOIN browser_kinds ON browser_kinds.id = browser_sessions.id
WHERE browser_kinds.kind IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM connectors WHERE connectors.browser_session_id = browser_sessions.id);

DELETE FROM browser_sessions
WHERE NOT EXISTS (SELECT 1 FROM connectors WHERE connectors.browser_session_id = browser_sessions.id);

DROP TABLE browser_kinds;

-- One connector per sign-in.
DROP INDEX connectors_browser_session_kind;
DROP INDEX connectors_token_account_kind;
DROP INDEX connectors_jira_credential_kind;
DROP INDEX connectors_github_credential_kind;

CREATE UNIQUE INDEX connectors_browser_session ON connectors (browser_session_id);
CREATE UNIQUE INDEX connectors_token_account ON connectors (token_account_id);
CREATE UNIQUE INDEX connectors_jira_credential ON connectors (jira_credential_id);
CREATE UNIQUE INDEX connectors_github_credential ON connectors (github_credential_id);
