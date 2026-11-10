// Copyright © 2026 Jalapeno Labs

//! `manager_make_changes` against a real Postgres and faked trackers: a Project Manager's
//! changes wait for review by default and apply at once when its project lets it write, each
//! is recorded on the project's timeline with the record that caused it, its issues file in the
//! project's tracker, and a retried turn makes nothing twice.

use serde_json::json;

use super::*;
use crate::action_items::Actor;
use crate::action_items::links::tests::LinkFixture;
use crate::action_items::watcher::pass;
use crate::models::action_item::{self, ActionItemState};
use crate::models::action_item_link::{LinkKind, LinkProvider};
use crate::models::attribution::{self, Attributor};
use crate::models::changeset::ChangesetState;
use crate::models::project::NewProject;
use crate::models::project_autonomy::{self, Autonomy};
use crate::models::project_issue_tracker::{self, NewTracker};
use crate::models::timeline_record::{self, NewTimelineRecord, TimelineRecord};
use crate::project_managers::trackers::NO_TRACKER;
use crate::test_support::{TEST_PERSON, TEST_PERSON_ID, test_member};

/// A project, a meeting segment on its timeline, and the trackers' fakes.
struct Fixture {
    links: LinkFixture,
    project_id: Uuid,
    segment: Uuid,
}

async fn fixture() -> Fixture {
    let mut links = LinkFixture::start().await;
    let project_id = project::create(
        &mut links.connection,
        &NewProject {
            created_by: TEST_PERSON_ID,
            name: "Pricing".to_owned(),
            description: String::new(),
        },
    )
    .await
    .expect("project")
    .id;
    let segment = record_on(&mut links.connection, project_id, "Annual pricing").await;
    Fixture {
        links,
        project_id,
        segment,
    }
}

/// A meeting's segment Elysia attributed to `project_id`.
async fn record_on(connection: &mut AsyncPgConnection, project_id: Uuid, title: &str) -> Uuid {
    let stored = timeline_record::insert_new(
        connection,
        NewTimelineRecord {
            id: Uuid::now_v7(),
            source_kind: "meeting".to_owned(),
            connector_id: None,
            external_id: Some(format!("segment:{}", Uuid::now_v7())),
            occurred_at: Utc::now(),
            actor: "Ada".to_owned(),
            title: title.to_owned(),
            body: "Ada: can we show annual pricing on the page?".to_owned(),
            url: None,
            media: None,
            metadata: json!({}),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("stored")
    .expect("new");
    attribution::attribute(connection, stored.id, project_id, 0.9, Attributor::Elysia)
        .await
        .expect("attributed");
    stored.id
}

/// The project names Jira's `ELY` as its tracker.
async fn names_jira(fixture: &mut Fixture) {
    project_issue_tracker::set(
        &mut fixture.links.connection,
        &NewTracker {
            project_id: fixture.project_id,
            provider: LinkProvider::Jira,
            jira_credential_id: Some(fixture.links.jira_credential.id),
            token_account_id: None,
            destination: "ELY".to_owned(),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("named");
}

/// Turns the project's dial to Full autonomy, the one level whose changes apply at once.
async fn lets_it_write(fixture: &mut Fixture) {
    let person = test_member(&mut fixture.links.connection).await;
    project_autonomy::set(
        &mut fixture.links.connection,
        fixture.project_id,
        Autonomy::FullAutonomy,
        &person,
    )
    .await
    .expect("turned up");
}

fn drafted(change: Value, cause_record_id: Uuid) -> Drafted {
    Drafted {
        draft: parse_change(change).expect("a change a Project Manager may make"),
        reason: "Ada asked for it in the pricing meeting.".to_owned(),
        quote: Some("Can we show annual pricing on the page?".to_owned()),
        source: None,
        cause_record_id,
    }
}

/// An item for annual pricing, with its issue filed: what a meeting's ask becomes.
fn file_annual_pricing(cause: Uuid) -> Vec<Drafted> {
    vec![
        drafted(
            json!({ "kind": "create-item", "title": "Annual pricing" }),
            cause,
        ),
        drafted(
            json!({ "kind": "file-issue", "item": { "operation": 1 } }),
            cause,
        ),
    ]
}

async fn make(fixture: &mut Fixture, drafts: Vec<Drafted>) -> Result<Made, Failure> {
    make_changes(
        &mut fixture.links.connection,
        fixture.project_id,
        "Follow-ups from the pricing meeting".to_owned(),
        drafts,
        Utc::now(),
    )
    .await
}

/// The project's records of its Project Manager's writes, oldest first.
async fn writes_on_timeline(fixture: &mut Fixture) -> Vec<TimelineRecord> {
    let mut records: Vec<TimelineRecord> =
        timeline_record::attributed_to(&mut fixture.links.connection, fixture.project_id, None, 50)
            .await
            .expect("timeline")
            .into_iter()
            .map(|(record, _attribution)| record)
            .filter(|record| record.metadata["kind"] == json!("write"))
            .collect();
    records.reverse();
    records
}

fn refusal(failure: Failure) -> String {
    let Failure::Refused(ToolError::Invalid(message)) = failure else {
        panic!("a refusal the Project Manager reads, not {failure:?}");
    };
    message
}

#[test]
fn a_project_manager_files_issues_but_never_names_a_tracker_or_links_by_hand() {
    let filed = parse_change(json!({ "kind": "file-issue", "item": { "operation": 1 } }))
        .expect("an item's issue");
    assert!(matches!(filed, Draft::FileIssue { .. }));

    parse_change(json!({
        "kind": "file-issue",
        "item": { "operation": 1 },
        "tracker": { "provider": "jira", "credentialId": Uuid::nil(), "destination": "OPS" },
    }))
    .expect_err("the project's tracker is the project's to decide");
    let link = parse_change(json!({
        "kind": "link",
        "item": { "id": Uuid::nil() },
        "target": { "provider": "jira", "credentialId": Uuid::nil(), "kind": "issue", "reference": "ELY-1" },
    }));
    assert!(link.is_err_and(|refusal| refusal.contains("file-issue")));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn by_default_its_changes_wait_for_review_and_apply_with_their_cause_once_approved() {
    let mut fixture = fixture().await;
    let segment = fixture.segment;
    names_jira(&mut fixture).await;

    let Made::Staged(staged) = make(&mut fixture, file_annual_pricing(segment))
        .await
        .expect("made")
    else {
        panic!("a project starts by having its Project Manager propose");
    };
    assert_eq!(staged.changeset.state, ChangesetState::Pending);
    assert_eq!(
        staged.operations[1].operation["tracker"],
        json!({
            "provider": "jira",
            "credentialId": fixture.links.jira_credential.id,
            "destination": "ELY",
        }),
        "the review shows where the issue goes"
    );
    assert!(
        staged
            .operations
            .iter()
            .all(|row| row.cause_record_id == Some(fixture.segment))
    );
    assert!(
        writes_on_timeline(&mut fixture).await.is_empty(),
        "nothing is written before a person approves"
    );

    // The turn is retried and proposes the same item from the same segment.
    let again = make(&mut fixture, file_annual_pricing(segment))
        .await
        .expect_err("made once");
    let message = refusal(again);
    assert!(
        message.contains(&staged.changeset.id.to_string()),
        "names where it already is: {message}"
    );

    let id = staged.changeset.id;
    changeset::decide(
        &mut fixture.links.connection,
        id,
        ChangesetDecision::Approved,
        None,
    )
    .await
    .expect("approved");
    let applied = changeset::apply(
        &mut fixture.links.connection,
        id,
        LinkReads::new(),
        TEST_PERSON_ID,
        Utc::now(),
    )
    .await
    .expect("applied");
    let item_id: Uuid =
        serde_json::from_value(applied.staged.operations[0].result["itemId"].clone())
            .expect("an item");
    assert_eq!(
        action_item::find(&mut fixture.links.connection, item_id)
            .await
            .expect("item")
            .state,
        ActionItemState::Open,
        "a person approving it accepts it"
    );

    let writes = writes_on_timeline(&mut fixture).await;
    let titles: Vec<&str> = writes.iter().map(|record| record.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Created an item: Annual pricing",
            "Filed in Jira ELY: Annual pricing"
        ]
    );
    for record in &writes {
        assert_eq!(record.metadata["causeRecordId"], json!(fixture.segment));
        assert_eq!(record.metadata["reviewed"], json!(true));
        assert!(
            record
                .body
                .contains("> Can we show annual pricing on the page?")
        );
    }
    let filing = action_item_issue_filing::for_item(&mut fixture.links.connection, item_id)
        .await
        .expect("read")
        .expect("owed");
    assert_eq!(filing.timeline_record_id, Some(writes[1].id));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_project_that_lets_it_write_applies_its_changes_at_once_and_files_the_issue() {
    let mut fixture = fixture().await;
    let segment = fixture.segment;
    names_jira(&mut fixture).await;
    lets_it_write(&mut fixture).await;

    let mut drafts = file_annual_pricing(segment);
    drafts.push(drafted(
        json!({
            "kind": "comment",
            "item": { "operation": 1 },
            "body": "A worker is on it.",
        }),
        segment,
    ));
    let Made::Applied(written) = make(&mut fixture, drafts).await.expect("made") else {
        panic!("a project that lets it write applies at once");
    };
    assert_eq!(written.staged.changeset.state, ChangesetState::Applied);
    assert_eq!(
        written.staged.changeset.decided_by,
        Some(PROJECT_MANAGER_USER_ID),
        "applied by the Project Managers' machine, not a person"
    );
    let item_id: Uuid =
        serde_json::from_value(written.staged.operations[0].result["itemId"].clone())
            .expect("an item");
    assert_eq!(
        action_item::find(&mut fixture.links.connection, item_id)
            .await
            .expect("item")
            .state,
        ActionItemState::Inbox,
        "nobody accepted it yet, so it waits in the inbox"
    );
    let writes = writes_on_timeline(&mut fixture).await;
    assert_eq!(writes.len(), 3, "every change it made, with its cause");
    assert!(
        writes
            .iter()
            .all(|record| record.metadata["reviewed"] == json!(false))
    );

    pass(&fixture.links.context()).await;
    let links = action_item_link::list_for_item(&mut fixture.links.connection, item_id)
        .await
        .expect("links");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].external_key, "ELY-1");
    assert!(links[0].is_primary);
    let filed = writes_on_timeline(&mut fixture).await.remove(1);
    assert_eq!(
        filed.url.as_deref(),
        Some("https://acme.atlassian.net/browse/ELY-1"),
        "the write's record links to the issue it filed"
    );

    // A person undoes it all: the item goes, and the issue already filed stays in Jira.
    let undone = changeset::undo(
        &mut fixture.links.connection,
        written.staged.changeset.id,
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("undone");
    let file_issue = &undone.staged.operations[1];
    assert_eq!(
        file_issue
            .undo
            .as_ref()
            .map(|undo| undo["stillFiled"]["key"].clone()),
        Some(json!("ELY-1"))
    );
    assert!(
        action_item_link::list_for_item(&mut fixture.links.connection, item_id)
            .await
            .expect("links")
            .is_empty(),
        "the item stops following the issue"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_change_names_a_record_on_its_project_and_an_issue_needs_somewhere_to_go() {
    let mut fixture = fixture().await;
    let segment = fixture.segment;
    let elsewhere = project::create(
        &mut fixture.links.connection,
        &NewProject {
            created_by: TEST_PERSON_ID,
            name: "Elsewhere".to_owned(),
            description: String::new(),
        },
    )
    .await
    .expect("project")
    .id;
    let their_record = record_on(&mut fixture.links.connection, elsewhere, "Not ours").await;

    let unrelated = make(
        &mut fixture,
        vec![drafted(
            json!({ "kind": "create-item", "title": "Annual pricing" }),
            their_record,
        )],
    )
    .await
    .expect_err("its cause is another project's");
    assert!(refusal(unrelated).contains("is not on this project's timeline"));

    let nowhere = make(&mut fixture, file_annual_pricing(segment))
        .await
        .expect_err("no tracker named, and no issue linked yet");
    let message = refusal(nowhere);
    assert!(message.contains(NO_TRACKER), "{message}");

    // An item linked to a YouTrack issue sets the precedent, so issues file there next.
    let remote = fixture
        .links
        .youtrack_provider()
        .find(fixture.links.youtrack_account.id, LinkKind::Issue, "JL-1")
        .await;
    assert!(
        remote.is_err(),
        "the fake holds nothing until a test puts it there"
    );
    fixture
        .links
        .put_youtrack_issue(crate::youtrack::tests::sample_issue("2-1", "JL-1", 1_000));
    let remote = fixture
        .links
        .youtrack_provider()
        .find(fixture.links.youtrack_account.id, LinkKind::Issue, "JL-1")
        .await
        .expect("found");
    action_item_link::create_linked_item(
        &mut fixture.links.connection,
        crate::models::action_item::NewActionItem {
            title: remote.title.clone(),
            notes: String::new(),
            state: ActionItemState::Open,
            priority: remote.starting_priority(),
            due_at: None,
            owner: remote.owner.clone(),
            project_ids: vec![fixture.project_id],
            initiative_ids: Vec::new(),
        },
        crate::models::action_item_link::NewLink {
            credential: fixture.links.youtrack_account,
            kind: LinkKind::Issue,
            external_id: remote.external_id.clone(),
            observation: remote.observation(),
        },
        Actor::User(TEST_PERSON_ID),
        Utc::now(),
    )
    .await
    .expect("linked");
    let Made::Staged(staged) = make(&mut fixture, file_annual_pricing(segment))
        .await
        .expect("made")
    else {
        panic!("proposed");
    };
    assert_eq!(
        staged.operations[1].operation["tracker"]["destination"],
        json!("JL"),
        "where the project's issues already are"
    );
}
