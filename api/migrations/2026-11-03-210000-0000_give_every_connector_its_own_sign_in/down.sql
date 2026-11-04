-- Undoes up.sql: an account can feed several connectors again, each kind once, and every
-- connector reads through one.
--
-- What up.sql split goes back together where it can be found again. Browsers it removed and
-- connectors it made for browsers no connector read are not undone: the shared layout allows
-- both, so they stay as they are.

DROP INDEX connectors_browser_session;
DROP INDEX connectors_token_account;
DROP INDEX connectors_jira_credential;
DROP INDEX connectors_github_credential;

ALTER TABLE connectors DROP CONSTRAINT connectors_one_sign_in;

-- A connector waiting for a browser reads through one of its person's browsers again: one
-- signed in to the same provider (Microsoft for Outlook and Teams) that does not already feed
-- its kind, the oldest first, which is the one up.sql took it from. One with no such browser
-- is removed, since every connector had an account; its timeline records stay, as for any
-- connector removed.
DO $$
DECLARE
    waiting RECORD;
    browser_id UUID;
BEGIN
    FOR waiting IN
        SELECT id, kind, created_by,
               CASE kind WHEN 'outlook' THEN 'microsoft' WHEN 'teams' THEN 'microsoft' ELSE kind END
                   AS provider
        FROM connectors
        WHERE num_nonnulls(browser_session_id, token_account_id, jira_credential_id, github_credential_id) = 0
        ORDER BY created_at, id
    LOOP
        SELECT browser_sessions.id INTO browser_id
        FROM browser_sessions
        WHERE browser_sessions.created_by = waiting.created_by
          AND NOT EXISTS (
              SELECT 1 FROM connectors
              WHERE connectors.browser_session_id = browser_sessions.id
                AND connectors.kind = waiting.kind
          )
        ORDER BY
            CASE substring(browser_sessions.start_url FROM '^https://([^/:?#]+)')
                WHEN 'outlook.office.com' THEN 'microsoft'
                WHEN 'outlook.office365.com' THEN 'microsoft'
                WHEN 'outlook.live.com' THEN 'microsoft'
                WHEN 'teams.cloud.microsoft' THEN 'microsoft'
                WHEN 'teams.microsoft.com' THEN 'microsoft'
                WHEN 'teams.live.com' THEN 'microsoft'
                WHEN 'mail.google.com' THEN 'gmail'
                WHEN 'app.slack.com' THEN 'slack'
            END IS NOT DISTINCT FROM waiting.provider DESC,
            browser_sessions.created_at,
            browser_sessions.id
        LIMIT 1;

        IF browser_id IS NULL THEN
            DELETE FROM connectors WHERE id = waiting.id;
        ELSE
            UPDATE connectors SET browser_session_id = browser_id WHERE id = waiting.id;
        END IF;
    END LOOP;
END
$$;

-- A connector on an Atlassian account without a Jira connector reads through the account its
-- site and address share with one again, and its own account is removed. One with no such
-- account keeps its own, which then carries a Jira connector as every Atlassian account did;
-- that account allows no Jira project or board, so its Jira connector reads nothing.
DO $$
DECLARE
    separate RECORD;
    shared_id UUID;
BEGIN
    FOR separate IN
        SELECT connectors.id AS connector_id,
               connectors.kind,
               jira_credentials.id AS credential_id,
               jira_credentials.site_url,
               jira_credentials.account_email,
               jira_credentials.created_by
        FROM connectors
        JOIN jira_credentials ON jira_credentials.id = connectors.jira_credential_id
        WHERE connectors.kind <> 'jira'
          AND NOT EXISTS (
              SELECT 1 FROM connectors AS jira
              WHERE jira.jira_credential_id = connectors.jira_credential_id
                AND jira.kind = 'jira'
          )
        ORDER BY connectors.created_at, connectors.id
    LOOP
        SELECT jira_credentials.id INTO shared_id
        FROM jira_credentials
        JOIN connectors AS jira
          ON jira.jira_credential_id = jira_credentials.id AND jira.kind = 'jira'
        WHERE jira_credentials.site_url = separate.site_url
          AND jira_credentials.account_email = separate.account_email
          AND NOT EXISTS (
              SELECT 1 FROM connectors
              WHERE connectors.jira_credential_id = jira_credentials.id
                AND connectors.kind = separate.kind
          )
        ORDER BY jira_credentials.created_at, jira_credentials.id
        LIMIT 1;

        IF shared_id IS NULL THEN
            INSERT INTO connectors (id, kind, jira_credential_id, created_by)
            VALUES (uuidv7(), 'jira', separate.credential_id, separate.created_by);
        ELSE
            UPDATE connectors SET jira_credential_id = shared_id WHERE id = separate.connector_id;
            DELETE FROM jira_credentials WHERE id = separate.credential_id;
        END IF;
    END LOOP;
END
$$;

CREATE UNIQUE INDEX connectors_browser_session_kind ON connectors (browser_session_id, kind);
CREATE UNIQUE INDEX connectors_token_account_kind ON connectors (token_account_id, kind);
CREATE UNIQUE INDEX connectors_jira_credential_kind ON connectors (jira_credential_id, kind);
CREATE UNIQUE INDEX connectors_github_credential_kind ON connectors (github_credential_id, kind);

ALTER TABLE connectors ADD CONSTRAINT connectors_one_account CHECK (
    num_nonnulls(browser_session_id, token_account_id, jira_credential_id, github_credential_id) = 1
);

DROP TABLE secret_copies;
