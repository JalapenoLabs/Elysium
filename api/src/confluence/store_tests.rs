// Copyright © 2026 Jalapeno Labs

//! Confluence against a real Postgres and the fake Confluence site: the reader end to end, and
//! that a Confluence connector's Atlassian account of its own never takes on Jira's reading.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use std::sync::Arc;

use chrono::Utc;
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::SecretString;
use serde_json::json;
use uuid::Uuid;

use super::reader::{Reader, ReaderDeps};
use super::tests::{ACCOUNT_ID, EMAIL, SITE_URL, TOKEN, fake_confluence_site};
use crate::connections;
use crate::connectors::ConnectorKind;
use crate::connectors::accounts;
use crate::connectors::activity::Activity;
use crate::crypto::Cipher;
use crate::database::schema::{action_items, timeline_records};
use crate::models::action_item::{ActionItemPriority, ActionItemState, NewActionItem, Owner};
use crate::models::action_item_link::{
    self, LinkCredential, LinkKind, LinkProvider, LinkState, NewLink, Observation,
};
use crate::models::connector::{self, AccountRef, Connector};
use crate::models::jira_credential::{self, Allowed, Allowlist, NewJiraCredential, VerifiedToken};
use crate::models::link_watch_cursor;
use crate::models::timeline_record::{self, NewTimelineRecord};
use crate::realtime::EventBus;
use crate::test_support::{TEST_PERSON, TEST_PERSON_ID, cipher, migrated_database};

/// An Atlassian account the fake Confluence accepts, named `name`.
fn new_account(name: &str, allowed: Allowed) -> NewJiraCredential {
    NewJiraCredential {
        name: name.to_owned(),
        verified: VerifiedToken {
            site_url: SITE_URL.to_owned(),
            account_email: EMAIL.to_owned(),
            token: SecretString::from(TOKEN),
            account: crate::jira::Account {
                account_id: ACCOUNT_ID.to_owned(),
                display_name: "Alex Navarro".to_owned(),
                email: None,
            },
        },
        allowed,
        created_by: TEST_PERSON_ID,
    }
}

/// An Atlassian account with the Jira connector it is made with.
async fn atlassian_account(connection: &mut AsyncPgConnection, cipher: &Cipher) -> Uuid {
    let allowed = Allowed {
        projects: Allowlist::All,
        boards: Allowlist::All,
    };
    jira_credential::create(connection, cipher, &new_account("Work", allowed))
        .await
        .expect("the account is created")
        .id
}

/// A Confluence connector with its own Atlassian account, which allows nothing of Jira's, reading
/// the ENG space, its first reads `baselined` or not.
async fn confluence_connector(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    baselined: bool,
) -> Connector {
    let allowed = Allowed {
        projects: Allowlist::Only(Vec::new()),
        boards: Allowlist::Only(Vec::new()),
    };
    let (_account, created) = jira_credential::create_carrying(
        connection,
        cipher,
        &new_account("Wiki", allowed),
        ConnectorKind::Confluence,
        json!({ "spaces": [{ "id": "1", "key": "ENG", "name": "Engineering" }] }),
    )
    .await
    .expect("the connector is created");
    if !baselined {
        return created;
    }
    connector::record_poll(
        connection,
        created.id,
        connector::PollOutcome {
            sign_in_state: "signed_in".to_owned(),
            cursor: json!({ "baselined": true, "through": Utc::now().to_rfc3339() }),
            last_polled_at: Some(Utc::now()),
            last_error: None,
        },
    )
    .await
    .expect("the cursor is set")
}

async fn reader(url: &SecretString, cipher: Arc<Cipher>) -> Reader {
    let database = connections::connect_postgres(url, 4)
        .await
        .expect("a pool on the test database");
    let events = EventBus::in_process();
    let (confluence, _log) = fake_confluence_site().await;
    Reader::new(ReaderDeps {
        activity: Activity::new(database.clone(), events.clone()),
        database,
        cipher,
        events,
        confluence,
    })
}

/// The external ids `connector_id` recorded, and how many raised an item.
async fn recorded(connection: &mut AsyncPgConnection, connector_id: Uuid) -> (Vec<String>, usize) {
    let rows: Vec<(Option<String>, Option<Uuid>)> = timeline_records::table
        .filter(timeline_records::connector_id.eq(connector_id))
        .order(timeline_records::external_id.asc())
        .select((
            timeline_records::external_id,
            timeline_records::action_item_id,
        ))
        .load(connection)
        .await
        .expect("records load");
    let raised = rows.iter().filter(|(_id, item)| item.is_some()).count();
    (
        rows.into_iter().filter_map(|(id, _item)| id).collect(),
        raised,
    )
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_read_records_each_change_in_its_spaces_and_raises_a_question_on_the_persons_page_once() {
    let (url, mut connection) = migrated_database().await;
    let cipher = Arc::new(cipher());
    let stored = confluence_connector(&mut connection, &cipher, true).await;
    let reader = reader(&url, Arc::clone(&cipher)).await;

    reader.read_now(&stored).await;

    // The fake answers a page and a comment in ENG and a page in SECRET, which is dropped.
    let (ids, raised) = recorded(&mut connection, stored.id).await;
    assert_eq!(ids, ["comment:201:1", "page:101:2"]);
    // The comment asks a question on a page the account's person created.
    assert_eq!(raised, 1);
    let title: String = action_items::table
        .select(action_items::title)
        .first(&mut connection)
        .await
        .expect("the raised item");
    assert_eq!(title, "Question on Re: Page 101");

    let reread = connector::find(&mut connection, stored.id)
        .await
        .expect("found");
    assert_eq!(reread.sign_in_state, "signed_in");
    assert_eq!(reread.last_error, None);
    reader.read_now(&reread).await;
    assert_eq!(
        recorded(&mut connection, stored.id).await,
        (ids, 1),
        "reading again records nothing"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_new_connectors_first_read_records_without_raising_and_baselines() {
    let (url, mut connection) = migrated_database().await;
    let cipher = Arc::new(cipher());
    let stored = confluence_connector(&mut connection, &cipher, false).await;
    let reader = reader(&url, Arc::clone(&cipher)).await;

    reader.read_now(&stored).await;

    let (ids, raised) = recorded(&mut connection, stored.id).await;
    assert_eq!(ids.len(), 2);
    assert_eq!(raised, 0);
    let reread = connector::find(&mut connection, stored.id)
        .await
        .expect("found");
    assert_eq!(reread.cursor["baselined"], json!(true));
}

/// The Jira connector the Atlassian account `account_id` was made with.
async fn jira_connector(connection: &mut AsyncPgConnection, account_id: Uuid) -> Connector {
    connector::find_by_account(connection, AccountRef::Atlassian(account_id))
        .await
        .expect("the query runs")
        .expect("an Atlassian account carries its Jira connector")
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn pausing_confluence_never_pauses_the_link_watcher() {
    let (_url, mut connection) = migrated_database().await;
    let cipher = cipher();
    let account_id = atlassian_account(&mut connection, &cipher).await;
    let confluence = confluence_connector(&mut connection, &cipher, true).await;

    connector::set_active(&mut connection, confluence.id, false)
        .await
        .expect("paused");
    assert!(
        connector::paused_link_accounts(&mut connection)
            .await
            .expect("listed")
            .is_empty()
    );

    let jira = jira_connector(&mut connection, account_id).await;
    connector::set_active(&mut connection, jira.id, false)
        .await
        .expect("paused");
    assert_eq!(
        connector::paused_link_accounts(&mut connection)
            .await
            .expect("listed"),
        [LinkCredential {
            provider: LinkProvider::Jira,
            id: account_id
        }]
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_confluence_account_is_never_offered_as_a_jira_site() {
    let (_url, mut connection) = migrated_database().await;
    let cipher = cipher();
    let account_id = atlassian_account(&mut connection, &cipher).await;
    let confluence = confluence_connector(&mut connection, &cipher, true).await;

    let listed: Vec<Uuid> = jira_credential::list(&mut connection)
        .await
        .expect("listed")
        .into_iter()
        .map(|(credential, _allowed)| credential.id)
        .collect();
    assert_eq!(listed, [account_id]);
    assert_ne!(confluence.jira_credential_id, Some(account_id));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_watchers_pass_and_link_counts_belong_to_jira_never_to_confluence() {
    let (_url, mut connection) = migrated_database().await;
    let cipher = cipher();
    let account_id = atlassian_account(&mut connection, &cipher).await;
    let confluence = confluence_connector(&mut connection, &cipher, true).await;
    let jira = jira_connector(&mut connection, account_id).await;
    let credential = LinkCredential {
        provider: LinkProvider::Jira,
        id: account_id,
    };
    link_watch_cursor::advance(&mut connection, credential, Utc::now())
        .await
        .expect("the watcher passed");
    link_watch_cursor::record_failure(&mut connection, credential, "Jira refused the token")
        .await
        .expect("the failure is kept");
    let listed = [confluence.clone(), jira.clone()];

    let passes = accounts::watcher_passes(&mut connection, &listed)
        .await
        .expect("passes load");
    assert!(passes.contains_key(&jira.id));
    assert!(!passes.contains_key(&confluence.id));

    // One issue linked through the account belongs to Jira; Confluence has recorded nothing.
    action_item_link::create_linked_item(
        &mut connection,
        NewActionItem {
            title: "Ship the hub".to_owned(),
            notes: String::new(),
            state: ActionItemState::Open,
            priority: ActionItemPriority::Normal,
            due_at: None,
            owner: Owner::User,
            project_ids: Vec::new(),
            initiative_ids: Vec::new(),
        },
        NewLink {
            credential,
            kind: LinkKind::Issue,
            external_id: "10042".to_owned(),
            observation: Observation {
                external_key: "ELY-42".to_owned(),
                url: format!("{SITE_URL}/browse/ELY-42"),
                title: "Ship the hub".to_owned(),
                state: LinkState::Open,
                owner: Owner::User,
            },
        },
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("the issue is linked");
    let counts = accounts::record_counts(&mut connection, &listed)
        .await
        .expect("counts load");
    assert_eq!(counts.get(&jira.id), Some(&1));
    assert!(!counts.contains_key(&confluence.id));
}

/// The records whose documents the index holds, by record id, each with its passages in order.
async fn indexed_records(connection: &mut AsyncPgConnection) -> Vec<(Uuid, String)> {
    use crate::database::schema::{knowledge_documents, knowledge_passages};

    let rows: Vec<(Option<Uuid>, String)> = knowledge_documents::table
        .inner_join(knowledge_passages::table)
        .order((
            knowledge_documents::timeline_record_id.asc(),
            knowledge_passages::position.asc(),
        ))
        .select((
            knowledge_documents::timeline_record_id,
            knowledge_passages::content,
        ))
        .load(connection)
        .await
        .expect("the index is read");
    rows.into_iter()
        .map(|(record_id, content)| (record_id.expect("a record's document"), content))
        .collect()
}

/// Version `external_id` of a Confluence page or comment, read by `connector_id`.
fn version(connector_id: Uuid, external_id: &str, body: &str) -> NewTimelineRecord {
    NewTimelineRecord {
        id: Uuid::now_v7(),
        source_kind: ConnectorKind::Confluence.as_str().to_owned(),
        connector_id: Some(connector_id),
        external_id: Some(external_id.to_owned()),
        occurred_at: Utc::now(),
        actor: "Grace".to_owned(),
        title: "Launch plan".to_owned(),
        body: body.to_owned(),
        url: None,
        media: None,
        metadata: json!({}),
        created_by: TEST_PERSON_ID,
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_index_holds_each_page_once_at_its_newest_version() {
    let (_url, mut connection) = migrated_database().await;
    let wiki = confluence_connector(&mut connection, &cipher(), true).await;
    let mut stored = Vec::new();
    for (external_id, body) in [
        ("page:42:1", "Ships Monday."),
        ("comment:43:1", "Looks good."),
        ("page:42:2", "Ships Friday."),
        // The same version read again, as an overlapping read does, is recorded once.
        ("page:42:2", "Ships Friday."),
        ("page:42:4", "Shipped."),
        // An older version read after a newer one stays out of the index.
        ("page:42:3", "Ships today."),
    ] {
        let record = version(wiki.id, external_id, body);
        stored.push(
            timeline_record::insert_new(&mut connection, record)
                .await
                .expect("stored")
                .map(|record| record.id),
        );
    }
    assert_eq!(stored[3], None, "a version is recorded once");

    let page = |body: &str| format!("Launch plan\n\nFrom Grace\n\n{body}");
    let comment = stored[1].expect("the comment is new");
    let newest = stored[4].expect("the newest version is new");
    let mut expected = vec![(comment, page("Looks good.")), (newest, page("Shipped."))];
    expected.sort();
    assert_eq!(indexed_records(&mut connection).await, expected);
}
