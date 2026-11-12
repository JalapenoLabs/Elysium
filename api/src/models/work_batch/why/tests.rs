// Copyright © 2026 Jalapeno Labs

//! A batch's chain read from its provenance against Postgres: what asked for it, in order, the
//! issue and the mail behind the item it works on, the item, the batch, and its pull request;
//! found work led by the finding a person accepted; many batches read at once; and what is gone
//! left out.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use serde_json::json;

use super::*;
use crate::auth::MemberUser;
use crate::findings;
use crate::models::action_item::{self, ActionItemPriority, NewActionItem, Owner};
use crate::models::finding::{Cost, Reporter};
use crate::models::meeting::Reach;
use crate::models::project::{self, NewProject};
use crate::models::project_manager;
use crate::models::timeline_record::{self, NewTimelineRecord};
use crate::models::user::{self, PROJECT_MANAGER_USER_ID};
use crate::models::work_batch::pull_request::{self, NewPullRequest};
use crate::models::work_batch::{self, NewWorkBatch, Priority};
use crate::realtime::EventBus;
use crate::test_support::{TEST_PERSON, TEST_PERSON_ID, migrated_database, person_rooted};

/// The test person, a member reading the chains of batches of their own meetings.
fn owner() -> Reader {
    Reader {
        id: TEST_PERSON_ID,
        reach: Reach::Public,
    }
}

async fn project_named(connection: &mut AsyncPgConnection, name: &str) -> Uuid {
    project::create(
        connection,
        &NewProject {
            name: name.to_owned(),
            description: String::new(),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("a project")
    .id
}

async fn record(
    connection: &mut AsyncPgConnection,
    source_kind: &str,
    title: &str,
    body: &str,
    metadata: Value,
) -> Uuid {
    timeline_record::insert_new(
        connection,
        NewTimelineRecord {
            id: Uuid::now_v7(),
            source_kind: source_kind.to_owned(),
            connector_id: None,
            external_id: None,
            occurred_at: Utc::now(),
            actor: "Ada".to_owned(),
            title: title.to_owned(),
            body: body.to_owned(),
            url: None,
            media: None,
            metadata,
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("stored")
    .expect("new")
    .id
}

async fn item(connection: &mut AsyncPgConnection, project_id: Uuid, title: &str) -> Uuid {
    action_item::create(
        connection,
        NewActionItem {
            title: title.to_owned(),
            notes: String::new(),
            state: ActionItemState::Open,
            priority: ActionItemPriority::Normal,
            due_at: None,
            owner: Owner::User,
            project_ids: vec![project_id],
            initiative_ids: Vec::new(),
        },
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("created")
    .record
    .id
}

/// Links `item_id` to GitHub issue `acme/web#number` as its primary, as the watcher would.
async fn link_issue(connection: &mut AsyncPgConnection, item_id: Uuid, number: u32) {
    diesel::sql_query(format!(
        "INSERT INTO github_credentials (id, name, kind, token_encrypted, login, scopes, \
         checked_at, created_by) VALUES ('{credential}', 'Token {number}', 'classic', \
         decode(repeat('ab', 60), 'hex'), 'ada', 'repo', now(), '{TEST_PERSON_ID}')",
        credential = Uuid::now_v7(),
    ))
    .execute(connection)
    .await
    .expect("a credential");
    diesel::sql_query(format!(
        "INSERT INTO action_item_links (id, action_item_id, provider, kind, github_credential_id, \
         external_id, external_key, url, title, is_primary, observed_state, observed_owner_kind) \
         SELECT '{link}', '{item_id}', 'github', 'issue', id, 'acme/web#{number}', \
         'acme/web#{number}', 'https://github.com/acme/web/issues/{number}', 'Annual pricing', \
         TRUE, 'open', 'user' FROM github_credentials WHERE name = 'Token {number}'",
        link = Uuid::now_v7(),
    ))
    .execute(connection)
    .await
    .expect("a link");
}

async fn submitted(connection: &mut AsyncPgConnection, batch: NewWorkBatch) -> WorkBatch {
    work_batch::submit(connection, batch)
        .await
        .expect("submitted")
        .batch
}

fn new_batch(project_id: Uuid, title: &str) -> NewWorkBatch {
    NewWorkBatch {
        id: Uuid::now_v7(),
        project_id,
        title: title.to_owned(),
        goal: "Do the work.".to_owned(),
        repositories: json!([]),
        priority: Priority::Normal,
        scheduled_for: None,
        turn_id: None,
        source_record_ids: json!([]),
        action_item_ids: json!([]),
        source_message_ids: json!([]),
        serves: None,
        lineage: person_rooted(),
        created_by: PROJECT_MANAGER_USER_ID,
    }
}

/// The kind of each link, as clients read it.
fn kinds(why: &Why) -> Vec<String> {
    why.chain
        .iter()
        .map(|link| {
            serde_json::to_value(link).expect("serializes")["kind"]
                .as_str()
                .expect("a kind")
                .to_owned()
        })
        .collect()
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_chain_runs_from_what_asked_to_the_pull_request() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    // A segment names its meeting, whose readers alone find its document.
    let (meeting, _capture) = crate::models::meeting::tests::stored(&mut connection).await;
    let segment = record(
        &mut connection,
        "meeting",
        "Annual pricing",
        "[00:12] Ada: We need annual pricing",
        json!({ "meetingId": meeting.id, "meetingTitle": "Weekly sync" }),
    )
    .await;
    let pricing = item(&mut connection, launch, "Annual pricing").await;
    link_issue(&mut connection, pricing, 12).await;
    let mail = record(
        &mut connection,
        "outlook",
        "Pricing for the launch",
        "Can we have annual pricing?",
        json!({}),
    )
    .await;
    timeline_record::set_action_item(&mut connection, mail, pricing)
        .await
        .expect("raised");
    let asked =
        project_manager::wake_with_message(&mut connection, launch, "Ship it.", TEST_PERSON_ID)
            .await
            .expect("woken")
            .id;

    let batch = submitted(
        &mut connection,
        NewWorkBatch {
            source_record_ids: json!([segment]),
            action_item_ids: json!([pricing]),
            source_message_ids: json!([asked]),
            ..new_batch(launch, "Ship annual pricing")
        },
    )
    .await;
    pull_request::record(
        &mut connection,
        NewPullRequest {
            work_batch_id: batch.id,
            repository: "acme/web",
            number: 7,
            url: "https://github.com/acme/web/pull/7",
            title: "Annual pricing",
        },
    )
    .await
    .expect("recorded");

    let why = read(&mut connection, &batch, owner()).await.expect("reads");
    assert_eq!(
        kinds(&why),
        [
            "message",
            "record",
            "issue",
            "record",
            "item",
            "batch",
            "pullRequest"
        ],
        "{why:#?}"
    );
    let Link::Record(quoted) = &why.chain[1] else {
        panic!("the segment second");
    };
    assert_eq!(quoted.id, segment);
    assert_eq!(quoted.excerpt, "[00:12] Ada: We need annual pricing");
    assert_eq!(quoted.meeting_title.as_deref(), Some("Weekly sync"));
    assert!(quoted.document_id.is_some(), "indexed as it was stored");
    let Link::Issue(issue) = &why.chain[2] else {
        panic!("the item's issue");
    };
    assert_eq!(issue.key, "acme/web#12");
    let Link::Record(raised) = &why.chain[3] else {
        panic!("the mail that raised the item");
    };
    assert_eq!(raised.id, mail);
    assert_eq!(why.batch().id, batch.id);
    assert!(
        matches!(why.head(), Some(Link::Message(message)) if message.id == asked),
        "a batch's chain starts from what asked for it"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_private_meetings_quote_is_withheld_from_whoever_may_not_read_it() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let (meeting, _capture) = crate::models::meeting::tests::stored(&mut connection).await;
    let segment = record(
        &mut connection,
        "meeting",
        "Grace's quarter",
        "[00:12] Alex: Grace is behind this quarter",
        json!({ "meetingId": meeting.id, "meetingTitle": "Alex and Grace" }),
    )
    .await;
    let unnamed = record(
        &mut connection,
        "meeting",
        "No meeting named",
        "[00:20] Alex: This names no meeting",
        json!({ "meetingTitle": "Somewhere" }),
    )
    .await;
    let batch = submitted(
        &mut connection,
        NewWorkBatch {
            source_record_ids: json!([segment, unnamed]),
            ..new_batch(launch, "Plan the quarter")
        },
    )
    .await;
    let someone_else = Reader {
        id: Uuid::now_v7(),
        reach: Reach::Public,
    };
    let quoted = |why: &Why| -> Vec<Record> {
        why.chain
            .iter()
            .filter_map(|link| match link {
                Link::Record(record) => Some(record.clone()),
                _other => None,
            })
            .collect()
    };

    // Its owner reads the quote of their meeting, still private to them.
    let as_owner = read(&mut connection, &batch, owner()).await.expect("reads");
    let records = quoted(&as_owner);
    assert!(!records[0].withheld);
    assert_eq!(
        records[0].excerpt,
        "[00:12] Alex: Grace is behind this quarter"
    );
    // Any other member is told a meeting asked, and nothing it said or is called, nor where it is.
    let as_member = read(&mut connection, &batch, someone_else)
        .await
        .expect("reads");
    assert_eq!(
        kinds(&as_member),
        kinds(&as_owner),
        "the chain keeps its shape"
    );
    let records = quoted(&as_member);
    assert!(records[0].withheld);
    assert_eq!(records[0].id, segment);
    assert_eq!(
        (
            records[0].title.as_str(),
            records[0].excerpt.as_str(),
            records[0].meeting_title.as_deref(),
            records[0].document_id
        ),
        ("", "", None, None)
    );
    // A segment naming no meeting is withheld from everyone, its owner included.
    assert!(quoted(&as_owner)[1].withheld);
    // Open to the company, every member reads its quote; text leaving Elysium names it too.
    meeting::set_visibility(&mut connection, meeting.id, meeting::Visibility::Public)
        .await
        .expect("opens");
    let opened = read(&mut connection, &batch, someone_else)
        .await
        .expect("reads");
    assert!(!quoted(&opened)[0].withheld);
    let described = read(&mut connection, &batch, Reader::ANY_MEMBER)
        .await
        .expect("reads");
    assert_eq!(
        quoted(&described)[0].meeting_title.as_deref(),
        Some("Alex and Grace")
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn many_chains_read_at_once_and_what_is_gone_is_left_out() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let website = project_named(&mut connection, "Website").await;
    let pricing = item(&mut connection, launch, "Annual pricing").await;
    let gone = item(&mut connection, launch, "Gone").await;
    diesel::update(action_items::table.find(gone))
        .set(action_items::deleted_at.eq(Utc::now()))
        .execute(&mut connection)
        .await
        .expect("deleted");
    let elsewhere =
        project_manager::wake_with_message(&mut connection, website, "Not this.", TEST_PERSON_ID)
            .await
            .expect("woken")
            .id;
    let first = submitted(
        &mut connection,
        NewWorkBatch {
            action_item_ids: json!([pricing, gone]),
            ..new_batch(launch, "Pricing")
        },
    )
    .await;
    let second = submitted(
        &mut connection,
        NewWorkBatch {
            action_item_ids: json!([pricing]),
            source_message_ids: json!([elsewhere]),
            ..new_batch(launch, "Docs")
        },
    )
    .await;

    let whys = read_many(&mut connection, &[&first, &second], owner())
        .await
        .expect("reads");
    assert_eq!(
        kinds(&whys[&first.id]),
        ["item", "batch"],
        "the deleted item is left out"
    );
    assert_eq!(
        kinds(&whys[&second.id]),
        ["item", "batch"],
        "another project's message names nothing"
    );
    assert!(
        matches!(whys[&second.id].head(), Some(Link::Item(item)) if item.id == pricing),
        "with nothing that asked, a chain starts from its item"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn found_work_leads_with_its_finding_and_who_accepted_it() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let events = EventBus::in_process();
    let reporting = submitted(&mut connection, new_batch(launch, "Annual pricing")).await;
    let reported = findings::report(
        &mut connection,
        &events,
        launch,
        Reporter::Worker {
            batch_id: reporting.id,
        },
        findings::Report {
            title: "Retry forever".to_owned(),
            detail: "The retry loop never gives up when the provider answers 429.".to_owned(),
            repository_url: None,
            location: "api/src/retry.rs".to_owned(),
            cost: Cost::Small,
        },
    )
    .await
    .expect("reported");
    let person = MemberUser::check(
        user::find(&mut connection, TEST_PERSON_ID)
            .await
            .expect("the test person"),
    )
    .expect("a member");
    let accepted = findings::accept(&mut connection, reported.finding.id, &person, Utc::now())
        .await
        .expect("accepted");
    let found = accepted.batch.batch();

    let why = read(&mut connection, found, owner()).await.expect("read");
    assert_eq!(kinds(&why), ["finding", "item", "batch"]);
    let Link::Finding(finding) = &why.chain[0] else {
        panic!("the chain leads with its finding: {:?}", why.chain);
    };
    assert_eq!(finding.id, reported.finding.id);
    assert_eq!(finding.title, "Retry forever");
    assert_eq!(finding.accepted_by_id, TEST_PERSON_ID);
    assert_eq!(finding.accepted_by, "Tester");
    assert_eq!(
        Some(finding.accepted_at),
        accepted.finding.decided_at,
        "accepted when the person decided"
    );

    // The batch that reported it is not the one accepting it made, so it names no finding.
    let reporter_why = read(&mut connection, &reporting, owner())
        .await
        .expect("read");
    assert_eq!(kinds(&reporter_why), ["batch"]);
}
