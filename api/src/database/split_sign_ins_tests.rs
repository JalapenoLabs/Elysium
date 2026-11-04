// Copyright © 2026 Jalapeno Labs

//! The migration that gives every connector its own sign-in (#138), against data laid out the
//! way production holds it before: Confluence on its Jira connector's Atlassian account, and
//! Outlook and Teams on one browser. Up must split them without losing a connector, and down
//! must share them again.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs it.

use diesel::QueryableByName;
use diesel::sql_types::{Bool, Nullable, Text, Uuid as SqlUuid};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl, SimpleAsyncConnection};
use secrecy::{ExposeSecret, SecretString};
use uuid::{Uuid, uuid};

use super::migrations::{self, MIGRATIONS, MigrationCommand};
use crate::test_support::empty_database;

/// The migration under test.
const SPLIT: &str = "2026-11-03-210000-0000_give_every_connector_its_own_sign_in";

const JIRA: Uuid = uuid!("00000000-0000-7000-8000-00000000c001");
const CONFLUENCE: Uuid = uuid!("00000000-0000-7000-8000-00000000c003");
const OUTLOOK: Uuid = uuid!("00000000-0000-7000-8000-00000000c101");
const TEAMS: Uuid = uuid!("00000000-0000-7000-8000-00000000c102");
const SLACK: Uuid = uuid!("00000000-0000-7000-8000-00000000c103");
const ATLASSIAN: Uuid = uuid!("00000000-0000-7000-8000-000000000a01");
const MICROSOFT_BROWSER: Uuid = uuid!("00000000-0000-7000-8000-0000000000b1");
const SLACK_BROWSER: Uuid = uuid!("00000000-0000-7000-8000-0000000000b2");
const GMAIL_BROWSER: Uuid = uuid!("00000000-0000-7000-8000-0000000000b3");
const OTHER_BROWSER: Uuid = uuid!("00000000-0000-7000-8000-0000000000b4");

/// The shared layout: an Atlassian account feeding Jira and Confluence, a browser feeding
/// Outlook and Teams, one feeding Slack alone, and two no connector reads, one on Gmail and one
/// on a page no kind reads.
const SHARED_LAYOUT: &str = "
INSERT INTO users (id, kratos_identity_id, email, name, role, status, approved_at)
VALUES ('0199a3c4-0000-7000-8000-00000000beef', '0199a3c4-0000-7000-8000-00000000cafe',
        'tester@example.com', 'Tester', 'admin', 'active', now());
INSERT INTO jira_credentials (id, name, site_url, account_email, token_encrypted, account_id,
                              all_projects, created_by)
VALUES ('00000000-0000-7000-8000-000000000a01', 'Work', 'https://acme.atlassian.net',
        'ada@acme.test', decode(repeat('ab', 60), 'hex'), 'acc-1', TRUE,
        '0199a3c4-0000-7000-8000-00000000beef');
INSERT INTO connectors (id, kind, jira_credential_id, content, created_by, created_at) VALUES
 ('00000000-0000-7000-8000-00000000c001', 'jira', '00000000-0000-7000-8000-000000000a01', '{}',
  '0199a3c4-0000-7000-8000-00000000beef', now() - interval '5 days'),
 ('00000000-0000-7000-8000-00000000c003', 'confluence', '00000000-0000-7000-8000-000000000a01',
  '{\"spaces\": \"*\"}', '0199a3c4-0000-7000-8000-00000000beef', now() - interval '4 days');
INSERT INTO browser_sessions (id, name, start_url, port, member_password_encrypted, created_by)
VALUES
 ('00000000-0000-7000-8000-0000000000b1', 'Work Outlook', 'https://outlook.office.com/mail/',
  59000, decode(repeat('ef', 60), 'hex'), '0199a3c4-0000-7000-8000-00000000beef'),
 ('00000000-0000-7000-8000-0000000000b2', 'Slack', 'https://app.slack.com/client',
  59001, decode(repeat('ef', 60), 'hex'), '0199a3c4-0000-7000-8000-00000000beef'),
 ('00000000-0000-7000-8000-0000000000b3', 'Gmail', 'https://mail.google.com/mail/',
  59002, decode(repeat('ef', 60), 'hex'), '0199a3c4-0000-7000-8000-00000000beef'),
 ('00000000-0000-7000-8000-0000000000b4', 'Docs', 'https://docs.example.com/',
  59003, decode(repeat('ef', 60), 'hex'), '0199a3c4-0000-7000-8000-00000000beef');
INSERT INTO connectors (id, kind, browser_session_id, cursor, content, created_by, created_at)
VALUES
 ('00000000-0000-7000-8000-00000000c101', 'outlook', '00000000-0000-7000-8000-0000000000b1',
  '{\"baselined\": true}', '{}', '0199a3c4-0000-7000-8000-00000000beef',
  now() - interval '3 days'),
 ('00000000-0000-7000-8000-00000000c102', 'teams', '00000000-0000-7000-8000-0000000000b1',
  '{\"baselined\": true, \"reader\": {\"since\": 5}}', '{\"conversations\": \"*\"}',
  '0199a3c4-0000-7000-8000-00000000beef', now() - interval '2 days'),
 ('00000000-0000-7000-8000-00000000c103', 'slack', '00000000-0000-7000-8000-0000000000b2',
  '{}', '{\"conversations\": \"*\"}', '0199a3c4-0000-7000-8000-00000000beef',
  now() - interval '2 days');
";

/// One connector's sign-in and state, as the test reads it back.
#[derive(Debug, PartialEq, Eq, QueryableByName)]
struct ConnectorRow {
    #[diesel(sql_type = Nullable<SqlUuid>)]
    browser_session_id: Option<Uuid>,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    jira_credential_id: Option<Uuid>,
    #[diesel(sql_type = Text)]
    sign_in_state: String,
    #[diesel(sql_type = Bool)]
    is_active: bool,
}

#[derive(QueryableByName)]
struct Found {
    #[diesel(sql_type = Bool)]
    found: bool,
}

async fn row_of(connection: &mut AsyncPgConnection, connector_id: Uuid) -> ConnectorRow {
    diesel::sql_query(
        "SELECT browser_session_id, jira_credential_id, sign_in_state, is_active \
         FROM connectors WHERE id = $1",
    )
    .bind::<SqlUuid, _>(connector_id)
    .get_result(connection)
    .await
    .expect("the connector is still there")
}

async fn holds(connection: &mut AsyncPgConnection, query: &str) -> bool {
    diesel::sql_query(format!("SELECT EXISTS ({query}) AS found"))
        .get_result::<Found>(connection)
        .await
        .expect("the query runs")
        .found
}

/// How many migrations come after [`SPLIT`], plus [`SPLIT`] itself, so reverting that many
/// lands just before it however many land after it.
fn from_split() -> u32 {
    let names: Vec<String> =
        diesel::migration::MigrationSource::<diesel::pg::Pg>::migrations(&MIGRATIONS)
            .expect("embedded migrations load")
            .iter()
            .map(|migration| migration.name().to_string())
            .collect();
    let position = names
        .iter()
        .position(|name| name == SPLIT)
        .expect("the split migration is embedded");
    u32::try_from(names.len() - position).expect("a handful of migrations")
}

/// A database just past the split, applied to [`SHARED_LAYOUT`].
async fn split_shared_layout() -> (SecretString, AsyncPgConnection) {
    let url = empty_database().await;
    migrations::execute(url.clone(), MigrationCommand::Run)
        .await
        .expect("up");
    migrations::execute(
        url.clone(),
        MigrationCommand::Revert {
            count: from_split(),
        },
    )
    .await
    .expect("back to just before the split");
    let mut connection = AsyncPgConnection::establish(url.expose_secret())
        .await
        .expect("connects");
    connection
        .batch_execute(SHARED_LAYOUT)
        .await
        .expect("the shared layout is stored");

    migrations::execute(url.clone(), MigrationCommand::Run)
        .await
        .expect("the split applies to shared data");
    (url, connection)
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_split_gives_every_connector_its_own_sign_in_and_loses_none() {
    let (_url, mut connection) = split_shared_layout().await;

    // Confluence has an Atlassian account of its own, a copy of Jira's that allows nothing of
    // Jira's, listed for the API to re-seal; Jira keeps the original.
    assert_eq!(
        row_of(&mut connection, JIRA).await.jira_credential_id,
        Some(ATLASSIAN)
    );
    let confluence = row_of(&mut connection, CONFLUENCE).await;
    let copy = confluence
        .jira_credential_id
        .expect("an account of its own");
    assert_ne!(copy, ATLASSIAN);
    assert!(
        holds(
            &mut connection,
            &format!(
                "SELECT 1 FROM jira_credentials WHERE id = '{copy}' AND NOT all_projects \
                 AND NOT all_boards AND site_url = 'https://acme.atlassian.net'"
            ),
        )
        .await
    );
    assert!(
        holds(
            &mut connection,
            &format!(
                "SELECT 1 FROM secret_copies WHERE row_id = '{copy}' AND sealed_for = '{ATLASSIAN}'"
            ),
        )
        .await
    );

    // The browser stays with Outlook, the kind its start page is for; Teams waits for its own,
    // signed out, and keeps its cursor.
    assert_eq!(
        row_of(&mut connection, OUTLOOK).await.browser_session_id,
        Some(MICROSOFT_BROWSER)
    );
    assert_eq!(
        row_of(&mut connection, TEAMS).await,
        ConnectorRow {
            browser_session_id: None,
            jira_credential_id: None,
            sign_in_state: "signed_out".to_owned(),
            is_active: true,
        }
    );
    assert!(
        holds(
            &mut connection,
            &format!(
                "SELECT 1 FROM connectors WHERE id = '{TEAMS}' AND cursor->'reader' IS NOT NULL"
            ),
        )
        .await
    );
    assert_eq!(
        row_of(&mut connection, SLACK).await.browser_session_id,
        Some(SLACK_BROWSER)
    );

    // The Gmail browser no connector read becomes a paused Gmail connector; the other is gone.
    assert!(
        holds(
            &mut connection,
            &format!(
                "SELECT 1 FROM connectors WHERE kind = 'gmail' \
                 AND browser_session_id = '{GMAIL_BROWSER}' AND NOT is_active"
            ),
        )
        .await
    );
    assert!(
        !holds(
            &mut connection,
            &format!("SELECT 1 FROM browser_sessions WHERE id = '{OTHER_BROWSER}'"),
        )
        .await
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn reverting_the_split_shares_the_sign_ins_again() {
    let (url, mut connection) = split_shared_layout().await;
    let copy = row_of(&mut connection, CONFLUENCE)
        .await
        .jira_credential_id
        .expect("an account of its own");

    migrations::execute(
        url.clone(),
        MigrationCommand::Revert {
            count: from_split(),
        },
    )
    .await
    .expect("the split reverts");

    // Shared again: Confluence back on Jira's account, its copy gone, Teams on Outlook's browser.
    assert_eq!(
        row_of(&mut connection, CONFLUENCE).await.jira_credential_id,
        Some(ATLASSIAN)
    );
    assert!(
        !holds(
            &mut connection,
            &format!("SELECT 1 FROM jira_credentials WHERE id = '{copy}'"),
        )
        .await
    );
    assert_eq!(
        row_of(&mut connection, TEAMS).await.browser_session_id,
        Some(MICROSOFT_BROWSER)
    );
}
