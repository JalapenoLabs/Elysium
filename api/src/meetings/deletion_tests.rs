// Copyright © 2026 Jalapeno Labs

//! Deleting meetings and restoring them, end to end against Postgres (#258): one or many at once,
//! each refusal named; a meeting's segments leaving every timeline and the knowledge index and
//! coming back with their attributions; its run stopping and carrying on; every source's next
//! listing leaving a deleted meeting deleted; and nothing changing it meanwhile. See
//! `docs/meetings.md`, Deleting.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::SecretString;
use serde_json::{Value, json};
use uuid::Uuid;

use super::deletion::{self, BulkChange, Outcome};
use super::fetch::FetchMeeting;
use super::fireflies::Fireflies;
use super::source::ListedMeeting;
use super::sources::Sources;
use super::{Meetings, MeetingsDeps, SEGMENT_SOURCE_KIND, correct, workflow};
use crate::action_items::Actor;
use crate::connectors::activity::Activity;
use crate::connectors::kinds::{self, Provides};
use crate::connectors::{AccountKind, ConnectorKind};
use crate::database::Pool;
use crate::database::schema::knowledge_documents;
use crate::meeting_uploads::staging::Staging;
use crate::models::action_item::{self, ActionItemPriority, ActionItemState, NewActionItem, Owner};
use crate::models::attribution::{self, Attributor};
use crate::models::browser_session::{self, NewBrowserSession};
use crate::models::connector::{self, AccountRef, Connector, NewConnector};
use crate::models::meeting::tests::stored;
use crate::models::meeting::{self, Meeting};
use crate::models::meeting_correction::{self, NewMeetingCorrection};
use crate::models::meeting_event::{self, EventKind};
use crate::models::project::{self, NewProject};
use crate::models::step_job::{self, StepJobState, StepOutcome};
use crate::models::timeline_record::{self, NewTimelineRecord, TimelineQuery};
use crate::models::token_account::{self, NewTokenAccount};
use crate::models::workflow_run::{self, WorkflowRunState};
use crate::realtime::EventBus;
use crate::runner::workspace::WorkspaceContainer;
use crate::test_support::{TEST_PERSON_ID, cipher, migrated_database};
use crate::workflows::{self, Step, StepContext};

/// Applies `change` to `meeting_ids` as the test person, as the bulk route does.
async fn change(
    connection: &mut AsyncPgConnection,
    mut meeting_ids: Vec<Uuid>,
    change: BulkChange,
) -> Outcome {
    meeting_ids.sort_unstable();
    deletion::change_many(
        connection,
        &meeting_ids,
        change,
        TEST_PERSON_ID,
        Utc::now(),
        None,
    )
    .await
    .expect("the change is written")
}

/// A segment of `meeting`, stored and indexed as segmenting publishes one, and put on `project_id`
/// by Elysia.
async fn segment_on(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    project_id: Uuid,
) -> Uuid {
    let record = NewTimelineRecord {
        id: Uuid::now_v7(),
        source_kind: SEGMENT_SOURCE_KIND.to_owned(),
        connector_id: None,
        external_id: Some(format!("meeting:{}#0", meeting.id)),
        occurred_at: meeting.started_at,
        actor: "Ada".to_owned(),
        title: "Launch date".to_owned(),
        body: "[0:00] Ada: The launch moves to Friday".to_owned(),
        url: None,
        media: Some(json!({ "meetingId": meeting.id, "startMs": 0, "endMs": 60_000 })),
        metadata: json!({ "meetingId": meeting.id, "summary": "The launch moves to Friday." }),
        created_by: meeting.created_by,
    };
    let stored = timeline_record::insert_new(connection, record)
        .await
        .expect("stores")
        .expect("a new record");
    attribution::attribute(connection, stored.id, project_id, 0.8, Attributor::Elysia)
        .await
        .expect("attributes");
    stored.id
}

async fn launch_project(connection: &mut AsyncPgConnection) -> Uuid {
    project::create(
        connection,
        &NewProject {
            name: "Launch".to_owned(),
            description: String::new(),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("a project")
    .id
}

/// Whether the knowledge index holds a document of record `record_id`.
async fn is_indexed(connection: &mut AsyncPgConnection, record_id: Uuid) -> bool {
    let documents: i64 = knowledge_documents::table
        .filter(knowledge_documents::timeline_record_id.eq(record_id))
        .count()
        .get_result(connection)
        .await
        .expect("reads the index");
    documents > 0
}

/// The ids of the records on project `project_id`'s timeline, as its Timeline tab lists them.
async fn on_timeline(connection: &mut AsyncPgConnection, project_id: Uuid) -> Vec<Uuid> {
    let query = TimelineQuery {
        owner: TEST_PERSON_ID,
        shared: true,
        project_id: Some(project_id),
        action_item_id: None,
        meeting_id: None,
        before: None,
        limit: 50,
    };
    timeline_record::list(connection, query)
        .await
        .expect("lists")
        .into_iter()
        .map(|(record, _)| record.id)
        .collect()
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_deleted_meetings_segments_leave_every_timeline_and_the_index_and_restoring_returns_them()
{
    let (_url, mut connection) = migrated_database().await;
    let project_id = launch_project(&mut connection).await;
    let (launch, _) = stored(&mut connection).await;
    let segment_id = segment_on(&mut connection, &launch, project_id).await;
    let (other, _) = stored(&mut connection).await;
    let kept_id = segment_on(&mut connection, &other, project_id).await;
    assert!(is_indexed(&mut connection, segment_id).await);

    let deleted = change(&mut connection, vec![launch.id], BulkChange::Delete {}).await;
    assert!(deleted.refused.is_empty(), "{:?}", deleted.refused);
    let [changed] = deleted.changed.as_slice() else {
        panic!("one meeting changed: {:?}", deleted.changed);
    };
    let deleted_at = changed.meeting.deleted_at.expect("it is deleted");
    assert_eq!(
        changed
            .records
            .iter()
            .map(|record| record.id)
            .collect::<Vec<_>>(),
        [segment_id],
        "its segment went with it, and only its own"
    );

    // It leaves every list but the deleted one, and its segment every reader.
    let listed: Vec<Uuid> = meeting::list_owned(&mut connection, TEST_PERSON_ID)
        .await
        .expect("lists")
        .iter()
        .map(|listed| listed.id)
        .collect();
    assert_eq!(listed, [other.id]);
    let deleted_list: Vec<Uuid> = meeting::list_deleted(&mut connection, TEST_PERSON_ID)
        .await
        .expect("lists")
        .iter()
        .map(|listed| listed.id)
        .collect();
    assert_eq!(deleted_list, [launch.id]);
    assert_eq!(on_timeline(&mut connection, project_id).await, [kept_id]);
    assert!(!is_indexed(&mut connection, segment_id).await);
    assert!(is_indexed(&mut connection, kept_id).await);
    assert!(matches!(
        timeline_record::find_owned(&mut connection, segment_id, TEST_PERSON_ID).await,
        Err(diesel::result::Error::NotFound)
    ));
    let read_by_manager = timeline_record::attributed_to(&mut connection, project_id, None, 50)
        .await
        .expect("reads");
    assert!(
        read_by_manager
            .iter()
            .all(|(record, _)| record.id != segment_id),
        "the project's Project Manager no longer reads it"
    );
    let projects = timeline_record::projects_of_meetings(&mut connection, &[launch.id])
        .await
        .expect("reads");
    assert!(projects.is_empty(), "its meeting is on no project");
    let walked = timeline_record::after_id(&mut connection, None, 100)
        .await
        .expect("walks");
    assert!(
        walked.iter().all(|record| record.id != segment_id),
        "the index's backfill walks past it, so it never comes back on its own"
    );
    // Its attribution waits for it.
    assert_eq!(
        timeline_record::attributions_of(&mut connection, &[segment_id])
            .await
            .expect("reads")
            .len(),
        1
    );
    let history = meeting_event::history(&mut connection, launch.id)
        .await
        .expect("history");
    let entry = history.last().expect("an entry");
    assert_eq!(entry.kind(), Some(EventKind::Deleted));
    assert_eq!(entry.created_by, TEST_PERSON_ID, "who deleted it is kept");
    assert_eq!(entry.data, json!({ "segments": 1, "connectorId": null }));

    let restored = change(&mut connection, vec![launch.id], BulkChange::Restore {}).await;
    assert!(restored.refused.is_empty(), "{:?}", restored.refused);
    assert_eq!(restored.changed[0].meeting.deleted_at, None);
    let mut on_project = on_timeline(&mut connection, project_id).await;
    on_project.sort_unstable();
    let mut both = vec![segment_id, kept_id];
    both.sort_unstable();
    assert_eq!(on_project, both, "its segment is on its project again");
    assert!(is_indexed(&mut connection, segment_id).await);
    let projects = timeline_record::projects_of_meetings(&mut connection, &[launch.id])
        .await
        .expect("reads");
    assert_eq!(projects.get(&launch.id), Some(&vec![project_id]));
    let history = meeting_event::history(&mut connection, launch.id)
        .await
        .expect("history");
    let entry = history.last().expect("an entry");
    assert_eq!(entry.kind(), Some(EventKind::Restored));
    assert_eq!(entry.created_by, TEST_PERSON_ID, "who restored it is kept");
    assert_eq!(
        entry.data,
        json!({ "deletedAt": deleted_at, "segments": 1, "runRetried": false })
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn many_meetings_change_at_once_and_each_one_refused_is_named() {
    let (_url, mut connection) = migrated_database().await;
    let (first, _) = stored(&mut connection).await;
    let (second, _) = stored(&mut connection).await;
    let (merged, _) = stored(&mut connection).await;
    let (kept, _) = stored(&mut connection).await;
    meeting::set_merged_into(&mut connection, merged.id, Some(kept.id))
        .await
        .expect("merged");
    let missing = Uuid::now_v7();

    let deleted = change(
        &mut connection,
        vec![first.id, second.id, merged.id, missing],
        BulkChange::Delete {},
    )
    .await;
    let mut changed: Vec<Uuid> = deleted
        .changed
        .iter()
        .map(|changed| changed.meeting.id)
        .collect();
    changed.sort_unstable();
    let mut expected = vec![first.id, second.id];
    expected.sort_unstable();
    assert_eq!(changed, expected);
    let refused: Vec<(Uuid, &str)> = deleted
        .refused
        .iter()
        .map(|refusal| (refusal.meeting_id, refusal.code))
        .collect();
    assert!(
        refused.contains(&(merged.id, "meeting_merged")),
        "{refused:?}"
    );
    assert!(refused.contains(&(missing, "not_found")), "{refused:?}");
    assert_eq!(refused.len(), 2);

    let again = change(&mut connection, vec![first.id], BulkChange::Delete {}).await;
    assert_eq!(again.refused[0].code, "meeting_deleted");
    let live = change(&mut connection, vec![kept.id], BulkChange::Restore {}).await;
    assert_eq!(live.refused[0].code, "meeting_not_deleted");

    let restored = change(
        &mut connection,
        vec![first.id, second.id],
        BulkChange::Restore {},
    )
    .await;
    assert_eq!(restored.changed.len(), 2);
    assert_eq!(
        meeting::list_owned(&mut connection, TEST_PERSON_ID)
            .await
            .expect("lists")
            .len(),
        3,
        "both are back beside the one kept; the merged one lives on in it"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_deleted_meeting_changes_no_further_and_starts_no_run() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let (deleted_meeting, _) = stored(&mut connection).await;
    change(
        &mut connection,
        vec![deleted_meeting.id],
        BulkChange::Delete {},
    )
    .await;

    let held = correct::hold(
        &mut connection,
        &events,
        deleted_meeting.id,
        TEST_PERSON_ID,
        true,
    )
    .await;
    assert!(matches!(held, Err(correct::HoldError::Deleted)), "{held:?}");
    let retried =
        workflow::retry(&mut connection, &events, deleted_meeting.id, TEST_PERSON_ID).await;
    assert!(
        matches!(retried, Err(step_job::StepActionError::Refused(_))),
        "{retried:?}"
    );
    let unfinished = meeting::list_unfinished(&mut connection)
        .await
        .expect("lists");
    assert!(
        unfinished
            .iter()
            .all(|waiting| waiting.id != deleted_meeting.id),
        "the meetings loop starts no run for it"
    );
}

// A deleted meeting asks nothing of its person, so the item already asking them to approve its
// corrected transcript goes with it; restoring leaves the correction waiting, as its row says.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn deleting_a_meeting_dismisses_the_item_asking_to_approve_its_transcript() {
    let (_url, mut connection) = migrated_database().await;
    let (held, _) = stored(&mut connection).await;
    meeting_correction::record(
        &mut connection,
        NewMeetingCorrection {
            meeting_id: held.id,
            transcript: json!([]),
            transcript_hash: "b".repeat(64),
            based_on: "source".to_owned(),
            source_transcript_hash: "a".repeat(64),
            whisper_transcript_hash: None,
            changed_lines: 1,
            fixes: json!([]),
        },
    )
    .await
    .expect("records")
    .expect("the first copy");
    let review = action_item::create(
        &mut connection,
        NewActionItem {
            title: "Approve the corrected transcript of Weekly sync".to_owned(),
            notes: String::new(),
            state: ActionItemState::Inbox,
            priority: ActionItemPriority::Normal,
            due_at: None,
            owner: Owner::User,
            project_ids: Vec::new(),
            initiative_ids: Vec::new(),
        },
        Actor::System,
        Utc::now(),
    )
    .await
    .expect("an item");
    meeting_correction::set_review_item(&mut connection, held.id, review.record.id)
        .await
        .expect("names it");

    let deleted = change(&mut connection, vec![held.id], BulkChange::Delete {}).await;
    let dismissed = deleted.changed[0].dismissed.as_ref().expect("dismissed");
    assert_eq!(dismissed.record.id, review.record.id);
    let item = action_item::find(&mut connection, review.record.id)
        .await
        .expect("found");
    assert_eq!(item.state, ActionItemState::Dismissed);

    // Restored, its correction still waits for approval, which the item no longer asks for.
    let restored = change(&mut connection, vec![held.id], BulkChange::Restore {}).await;
    assert!(restored.changed[0].dismissed.is_none());
    let correction = meeting_correction::find(&mut connection, held.id)
        .await
        .expect("reads")
        .expect("kept");
    assert_eq!(correction.approved_at, None);
}

/// One attempt at the run's next step, the fetch, which answers how it failed; recorded as the
/// engine's workers record it.
async fn fetch_next(
    connection: &mut AsyncPgConnection,
    database: &Pool,
) -> crate::workflows::StepFailure {
    let at = Utc::now();
    let claims = step_job::claim(connection, at, 1)
        .await
        .expect("claims")
        .claims;
    let [claim] = claims.as_slice() else {
        panic!("the fetch alone is due: {claims:?}");
    };
    assert_eq!(claim.job.step, FetchMeeting::NAME);
    let input = serde_json::from_value(claim.job.input.clone().expect("an input"))
        .expect("the step reads its input");
    let fetch = FetchMeeting::for_test(database.clone(), Arc::new(cipher()), unreachable_sources());
    let context = StepContext::for_test::<FetchMeeting>(database.clone());
    let failure = fetch
        .run(input, context)
        .await
        .expect_err("a deleted meeting is not fetched");
    step_job::finish(
        connection,
        claim,
        StepOutcome::Failed {
            reason: failure.reason.clone(),
            retryable: failure.retryable,
        },
        at,
    )
    .await
    .expect("records")
    .expect("the claim holds");
    failure
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn deleting_a_meeting_stops_its_run_and_restoring_it_carries_the_run_on() {
    let (url, mut connection) = migrated_database().await;
    let database = crate::connections::connect_postgres(&url, 4)
        .await
        .expect("a pool");
    let events = EventBus::in_process();
    let (pending, _) = stored(&mut connection).await;
    let started = workflows::start(&mut connection, &events, workflow::run_for(pending.id))
        .await
        .expect("the run starts");
    let run_id = started.settled.run.id;

    change(&mut connection, vec![pending.id], BulkChange::Delete {}).await;
    let failure = fetch_next(&mut connection, &database).await;
    assert_eq!(failure.reason, deletion::STOPPED);
    assert!(!failure.retryable, "it waits for its person to restore it");
    let run = workflow_run::find(&mut connection, run_id)
        .await
        .expect("run");
    assert_eq!(run.state, WorkflowRunState::Failed);

    let restored = change(&mut connection, vec![pending.id], BulkChange::Restore {}).await;
    assert_eq!(
        restored.changed[0].runs.len(),
        1,
        "the run it stopped goes on"
    );
    // A run whose step waits to be claimed again is running, as after any retry.
    let run = workflow_run::find(&mut connection, run_id)
        .await
        .expect("run");
    assert_eq!(run.state, WorkflowRunState::Running);
    let steps = workflow_run::steps_of(&mut connection, run_id)
        .await
        .expect("steps");
    assert_eq!(steps[0].state, StepJobState::Queued);
    let history = workflow_run::history(&mut connection, run_id)
        .await
        .expect("history");
    let retried = history.last().expect("an entry");
    assert_eq!(retried.kind, "retried");
    assert_eq!(
        retried.created_by, TEST_PERSON_ID,
        "as the person restoring it"
    );
    let entry = meeting_event::history(&mut connection, pending.id)
        .await
        .expect("history")
        .pop()
        .expect("an entry");
    assert_eq!(entry.data["runRetried"], json!(true));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_run_that_failed_before_the_deletion_waits_for_its_person_after_the_restore() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let (pending, _) = stored(&mut connection).await;
    let started = workflows::start(&mut connection, &events, workflow::run_for(pending.id))
        .await
        .expect("the run starts");
    let at = Utc::now();
    let claims = step_job::claim(&mut connection, at, 1)
        .await
        .expect("claims")
        .claims;
    step_job::finish(
        &mut connection,
        &claims[0],
        StepOutcome::Failed {
            reason: "Fireflies refused the key".to_owned(),
            retryable: false,
        },
        at,
    )
    .await
    .expect("records")
    .expect("the claim holds");

    change(&mut connection, vec![pending.id], BulkChange::Delete {}).await;
    let restored = change(&mut connection, vec![pending.id], BulkChange::Restore {}).await;
    assert!(restored.changed[0].runs.is_empty());
    let run = workflow_run::find(&mut connection, started.settled.run.id)
        .await
        .expect("run");
    assert_eq!(run.state, WorkflowRunState::Failed);
    assert_eq!(run.error.as_deref(), Some("Fireflies refused the key"));
}

/// Sources that answer nothing, for steps and listings that never reach one.
fn unreachable_sources() -> Sources {
    let endpoint = url::Url::parse("http://127.0.0.1:9/graphql").expect("a static address parses");
    Sources::with_fireflies(Fireflies::at(reqwest::Client::new(), endpoint))
}

/// The meetings loop over `database`, its sources and workspace answering nothing: enough to store
/// what a listing found.
fn meetings_over(database: Pool) -> Meetings {
    let events = EventBus::in_process();
    let docker =
        bollard::Docker::connect_with_http("http://127.0.0.1:9", 5, bollard::API_DEFAULT_VERSION)
            .expect("a static address parses");
    let workspace = WorkspaceContainer::new(docker, "elysium-workspace:test".to_owned());
    Meetings::new(MeetingsDeps {
        activity: Activity::new(database.clone(), events.clone()),
        database,
        events,
        cipher: Arc::new(cipher()),
        sources: unreachable_sources(),
        staging: Staging::in_workspace(workspace),
    })
}

/// A connector of `kind` of the test person's, signed in with the account its kind takes; a
/// browser kind's on media port `port`.
async fn connector_of(
    connection: &mut AsyncPgConnection,
    kind: ConnectorKind,
    port: i32,
) -> Connector {
    let account = match kinds::definition(kind).account {
        AccountKind::Browser => {
            let session = browser_session::create(
                connection,
                &cipher(),
                NewBrowserSession {
                    id: Uuid::now_v7(),
                    name: kind.as_str().to_owned(),
                    start_url: "https://example.com/".to_owned(),
                    port,
                    member_password: SecretString::from("member-password"),
                    created_by: TEST_PERSON_ID,
                },
            )
            .await
            .expect("a browser");
            AccountRef::Browser(session.id)
        }
        AccountKind::Fireflies => {
            let key = token_account::create(
                connection,
                NewTokenAccount::new(
                    &cipher(),
                    "fireflies",
                    "Fireflies",
                    &SecretString::from("fireflies-key"),
                    "ada@example.com",
                    TEST_PERSON_ID,
                ),
            )
            .await
            .expect("a key");
            AccountRef::Token(key.id)
        }
        other => panic!("no meeting source signs in with {other:?}"),
    };
    connector::create(
        connection,
        NewConnector::new(kind.as_str(), account, TEST_PERSON_ID),
    )
    .await
    .expect("a connector")
}

fn listing(external_id: &str, started_at: DateTime<Utc>) -> ListedMeeting {
    ListedMeeting {
        external_id: external_id.to_owned(),
        title: "Launch review".to_owned(),
        started_at,
        duration_ms: Some(1_800_000),
        organizer: "ada@example.com".to_owned(),
        participants: vec!["ada@example.com".to_owned()],
        url: None,
    }
}

/// The meeting holding connector `connector_id`'s capture `external_id`, if one does.
async fn meeting_of(
    connection: &mut AsyncPgConnection,
    connector_id: Uuid,
    external_id: &str,
) -> Option<Meeting> {
    use crate::database::schema::meeting_captures;

    // `meeting_captures_once_per_connector` holds one capture per connector and id at most.
    let meeting_id: Uuid = meeting_captures::table
        .filter(meeting_captures::connector_id.eq(connector_id))
        .filter(meeting_captures::external_id.eq(external_id))
        .select(meeting_captures::meeting_id)
        .first(connection)
        .await
        .optional()
        .expect("reads")?;
    Some(
        meeting::find(connection, meeting_id)
            .await
            .expect("its meeting"),
    )
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn every_sources_next_listing_leaves_a_deleted_meeting_deleted() {
    let (url, mut connection) = migrated_database().await;
    let database = crate::connections::connect_postgres(&url, 4)
        .await
        .expect("a pool");
    let meetings = meetings_over(database);
    let started_at = Utc::now() - chrono::TimeDelta::hours(2);

    // Every kind that provides meetings lists through the same store, whichever way it reads.
    // A browser kind takes a media port of its own, from the first nginx publishes.
    for (port, kind) in (59_000..).zip(kinds::providing(Provides::Meetings)) {
        let account = connector_of(&mut connection, kind, port).await;
        let listed = [listing("review-1", started_at)];
        let (_, stored_count) = meetings
            .store_listed(&account, &listed)
            .await
            .expect("stores the listing");
        assert_eq!(stored_count, 1, "{kind:?} stores a meeting it never read");
        let first = meeting_of(&mut connection, account.id, "review-1")
            .await
            .expect("stored");

        change(&mut connection, vec![first.id], BulkChange::Delete {}).await;
        let again = [
            listing("review-1", started_at),
            listing("review-2", started_at),
        ];
        let (_, stored_count) = meetings
            .store_listed(&account, &again)
            .await
            .expect("stores the listing");
        assert_eq!(
            stored_count, 1,
            "{kind:?} stores only the new meeting, never the deleted one again"
        );
        let still = meeting_of(&mut connection, account.id, "review-1")
            .await
            .expect("still there");
        assert_eq!(still.id, first.id, "{kind:?} keeps the one meeting it had");
        assert!(still.deleted_at.is_some(), "{kind:?} leaves it deleted");
        let listed_ids: Vec<Uuid> = meeting::list_owned(&mut connection, TEST_PERSON_ID)
            .await
            .expect("lists")
            .iter()
            .map(|listed| listed.id)
            .collect();
        assert!(!listed_ids.contains(&first.id), "{kind:?} lists it nowhere");
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_connector_names_only_the_live_meetings_it_listed_first() {
    let (url, mut connection) = migrated_database().await;
    let database = crate::connections::connect_postgres(&url, 4)
        .await
        .expect("a pool");
    let meetings = meetings_over(database);
    let started_at = Utc::now() - chrono::TimeDelta::hours(2);
    let fireflies = connector_of(&mut connection, ConnectorKind::Fireflies, 0).await;
    let teams = connector_of(&mut connection, ConnectorKind::Teams, 59_000).await;
    let listed = [
        listing("a", started_at),
        listing("b", started_at),
        listing("c", started_at),
    ];
    meetings
        .store_listed(&fireflies, &listed)
        .await
        .expect("stores");
    meetings
        .store_listed(&teams, &[listing("t", started_at)])
        .await
        .expect("stores");
    let deleted = meeting_of(&mut connection, fireflies.id, "c")
        .await
        .expect("stored");
    change(&mut connection, vec![deleted.id], BulkChange::Delete {}).await;

    let named = meeting::listed_by(&mut connection, fireflies.id, TEST_PERSON_ID)
        .await
        .expect("names");
    let mut expected = Vec::new();
    for external_id in ["a", "b"] {
        expected.push(
            meeting_of(&mut connection, fireflies.id, external_id)
                .await
                .expect("stored")
                .id,
        );
    }
    expected.sort_unstable();
    assert_eq!(named, expected, "the deleted one and Teams' are not its");
    let nobody_elses = meeting::listed_by(&mut connection, fireflies.id, Uuid::now_v7())
        .await
        .expect("names");
    assert!(
        nobody_elses.is_empty(),
        "only its person's meetings are named"
    );
}

/// What a request to the router answered, its status and its JSON body.
async fn answered(response: axum::response::Response) -> (axum::http::StatusCode, Value) {
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("the body reads");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "a connector removed alone and with its meetings, then those meetings over the route"
)]
async fn removing_a_connector_deletes_its_meetings_only_when_asked() {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use tower::ServiceExt as _;

    use crate::test_support::{app_state, signed_in_cookie};

    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let cookie = signed_in_cookie(&state).await;
    let meetings = meetings_over(state.database.clone());
    let started_at = Utc::now() - chrono::TimeDelta::hours(2);
    let send = async |method: Method, uri: String| {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, cookie.clone())
            .header(header::ORIGIN, "http://localhost:4000")
            .body(Body::empty())
            .expect("a request builds");
        let response = crate::routes::router(&state)
            .with_state(state.clone())
            .oneshot(request)
            .await
            .expect("the router answers");
        answered(response).await
    };

    // Removed alone, its meetings stay.
    let kept = connector_of(&mut connection, ConnectorKind::Fireflies, 0).await;
    meetings
        .store_listed(&kept, &[listing("stays", started_at)])
        .await
        .expect("stores");
    let (status, _) = send(Method::DELETE, format!("/api/v1/connectors/{}", kept.id)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let stays = meeting::list_owned(&mut connection, TEST_PERSON_ID)
        .await
        .expect("lists");
    assert_eq!(stays.len(), 1, "a meeting whose connector is gone stays");

    // Asked, they go with it, recorded as removed with it.
    let removed = connector_of(&mut connection, ConnectorKind::Fireflies, 0).await;
    meetings
        .store_listed(
            &removed,
            &[listing("one", started_at), listing("two", started_at)],
        )
        .await
        .expect("stores");
    let (status, count) = send(
        Method::GET,
        format!("/api/v1/connectors/{}/meeting-count", removed.id),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{count}");
    assert_eq!(count, json!({ "count": 2 }));
    let (status, _) = send(
        Method::DELETE,
        format!("/api/v1/connectors/{}?deleteMeetings=true", removed.id),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let deleted = meeting::list_deleted(&mut connection, TEST_PERSON_ID)
        .await
        .expect("lists");
    assert_eq!(deleted.len(), 2);
    for gone in &deleted {
        let entry = meeting_event::history(&mut connection, gone.id)
            .await
            .expect("history")
            .pop()
            .expect("an entry");
        assert_eq!(entry.kind(), Some(EventKind::Deleted));
        assert_eq!(entry.data["connectorId"], json!(removed.id));
    }
    let left: Vec<Uuid> = meeting::list_owned(&mut connection, TEST_PERSON_ID)
        .await
        .expect("lists")
        .iter()
        .map(|listed| listed.id)
        .collect();
    assert_eq!(
        left,
        [stays[0].id],
        "and the other connector's meeting stays"
    );

    // A meeting whose connector is gone is deleted and restored like any other, over the route.
    let (status, body) = send_json(
        &state,
        &cookie,
        json!({ "meetingIds": [stays[0].id, deleted[0].id], "change": { "kind": "delete" } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meetings"][0]["id"], json!(stays[0].id));
    assert!(body["meetings"][0]["deletedAt"].is_string());
    assert_eq!(
        body["refused"],
        json!([{
            "meetingId": deleted[0].id,
            "code": "meeting_deleted",
            "message": "this meeting is deleted already",
        }])
    );
    let (status, body) = send_json(
        &state,
        &cookie,
        json!({ "meetingIds": [stays[0].id], "change": { "kind": "restore" } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meetings"][0]["deletedAt"], Value::Null);
    let (status, listed) = send(Method::GET, "/api/v1/meetings?deleted=true".to_owned()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["meetings"].as_array().map(Vec::len), Some(2));
}

// The connector goes after its meetings, and a browser's removal reaches Docker, which answers
// nothing here: the removal fails, and the meetings it deleted come back, so it can be tried again.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_removal_that_fails_restores_the_meetings_it_deleted() {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use tower::ServiceExt as _;

    use crate::test_support::{app_state, signed_in_cookie};

    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let cookie = signed_in_cookie(&state).await;
    let meetings = meetings_over(state.database.clone());
    let started_at = Utc::now() - chrono::TimeDelta::hours(2);
    let teams = connector_of(&mut connection, ConnectorKind::Teams, 59_000).await;
    meetings
        .store_listed(
            &teams,
            &[listing("one", started_at), listing("two", started_at)],
        )
        .await
        .expect("stores");

    let request = Request::builder()
        .method(Method::DELETE)
        .uri(format!(
            "/api/v1/connectors/{}?deleteMeetings=true",
            teams.id
        ))
        .header(header::COOKIE, cookie)
        .header(header::ORIGIN, "http://localhost:4000")
        .body(Body::empty())
        .expect("a request builds");
    let response = crate::routes::router(&state)
        .with_state(state.clone())
        .oneshot(request)
        .await
        .expect("the router answers");
    let (status, body) = answered(response).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");

    connector::find(&mut connection, teams.id)
        .await
        .expect("the connector stays");
    let live = meeting::list_owned(&mut connection, TEST_PERSON_ID)
        .await
        .expect("lists");
    assert_eq!(live.len(), 2, "its meetings are back");
    for back in &live {
        let kinds: Vec<Option<EventKind>> = meeting_event::history(&mut connection, back.id)
            .await
            .expect("history")
            .iter()
            .map(meeting_event::MeetingEvent::kind)
            .collect();
        assert_eq!(kinds, [Some(EventKind::Deleted), Some(EventKind::Restored)]);
    }
}

/// The router's answer to a bulk change of meetings, as the test person.
async fn send_json(
    state: &crate::state::AppState,
    cookie: &str,
    body: Value,
) -> (axum::http::StatusCode, Value) {
    use axum::body::Body;
    use axum::http::{Method, Request, header};
    use tower::ServiceExt as _;

    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/meetings/bulk")
        .header(header::COOKIE, cookie)
        .header(header::ORIGIN, "http://localhost:4000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("a request builds");
    let response = crate::routes::router(state)
        .with_state(state.clone())
        .oneshot(request)
        .await
        .expect("the router answers");
    answered(response).await
}
