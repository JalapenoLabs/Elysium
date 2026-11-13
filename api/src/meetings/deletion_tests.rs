// Copyright © 2026 Jalapeno Labs

//! Deleting meetings and restoring them, end to end against Postgres (#258): one or many at once,
//! each refusal named; a meeting's excerpts and its transcript leaving every timeline and the
//! knowledge index and coming back with their attributions, a removed excerpt staying removed; its
//! review item dismissed and returned to where it stood; its run stopping and carrying on; every
//! source's next listing leaving a deleted meeting deleted; and nothing changing it meanwhile. See
//! `docs/meetings.md`, Deleting.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::SecretString;
use serde_json::{Value, json};
use uuid::Uuid;

use super::deletion::{self, BulkChange, Outcome};
use super::excerpts::{self, MeetingText};
use super::fetch::FetchMeeting;
use super::fireflies::Fireflies;
use super::segmentation::SegmentProject;
use super::source::{ListedMeeting, TranscriptLine};
use super::sources::Sources;
use super::{Meetings, MeetingsDeps, SEGMENT_SOURCE_KIND, correct, final_transcript, workflow};
use crate::action_items::Actor;
use crate::connectors::activity::Activity;
use crate::connectors::kinds::{self, Provides};
use crate::connectors::{AccountKind, ConnectorKind};
use crate::database::Pool;
use crate::database::schema::{knowledge_documents, meetings};
use crate::meeting_uploads::staging::Staging;
use crate::models::action_item::{self, ActionItemPriority, ActionItemState, NewActionItem, Owner};
use crate::models::agent::Author;
use crate::models::attribution::{self, Attributor};
use crate::models::browser_session::{self, NewBrowserSession};
use crate::models::connector::{self, AccountRef, Connector, NewConnector};
use crate::models::meeting::tests::{fetched_with, stored};
use crate::models::meeting::{self, Meeting, Reach, Reader};
use crate::models::meeting_correction::{self, NewMeetingCorrection};
use crate::models::meeting_event::{self, EventKind};
use crate::models::meeting_mark::Range;
use crate::models::meeting_word;
use crate::models::project::{self, NewProject};
use crate::models::step_job::{self, StepJobState, StepOutcome};
use crate::models::timeline_record::{self, NewTimelineRecord, TimelineQuery, TimelineRecord};
use crate::models::token_account::{self, NewTokenAccount};
use crate::models::user::{self, ELYSIA_USER_ID, User};
use crate::models::workflow_run::{self, WorkflowRunState};
use crate::realtime::EventBus;
use crate::runner::workspace::WorkspaceContainer;
use crate::test_support::{TEST_PERSON_ID, cipher, migrated_database};
use crate::workflows::{self, Step, StepContext};

/// The test person, who owns every meeting here and makes every change.
async fn test_person(connection: &mut AsyncPgConnection) -> User {
    user::find(connection, TEST_PERSON_ID)
        .await
        .expect("the test person")
}

/// Applies `change` to `meeting_ids` as the test person, as the bulk route does.
async fn change(
    connection: &mut AsyncPgConnection,
    mut meeting_ids: Vec<Uuid>,
    change: BulkChange,
) -> Outcome {
    meeting_ids.sort_unstable();
    let person = test_person(connection).await;
    deletion::change_many(connection, &meeting_ids, change, &person, Utc::now(), None)
        .await
        .expect("the change is written")
}

/// What the test person's Meetings page lists: the meetings they may read, or, `deleted`, the
/// deleted ones they may restore.
async fn meetings_page(connection: &mut AsyncPgConnection, deleted: bool) -> Vec<Meeting> {
    let reader = Reader::of(&test_person(connection).await);
    let page = if deleted {
        meeting::list_deleted(connection, reader, None, 200).await
    } else {
        meeting::list_readable(connection, reader, None, 200).await
    };
    page.expect("lists")
}

/// The test person, now.
fn person() -> Author<'static> {
    Author {
        user_id: TEST_PERSON_ID,
        agent: None,
        at: Utc::now(),
    }
}

/// A segmented meeting of the test person's, its final transcript lined up from three lines and
/// its whole transcript in the knowledge index, as segmenting leaves one.
async fn segmented(connection: &mut AsyncPgConnection) -> Meeting {
    let lines = [
        TranscriptLine {
            start_ms: 0,
            end_ms: 2_000,
            speaker: "Ada".to_owned(),
            text: "The launch moves to Friday.".to_owned(),
        },
        TranscriptLine {
            start_ms: 2_000,
            end_ms: 4_000,
            speaker: "Grace".to_owned(),
            text: "Then the docs ship Thursday.".to_owned(),
        },
        TranscriptLine {
            start_ms: 4_000,
            end_ms: 6_000,
            speaker: "Ada".to_owned(),
            text: "Lunch is on me.".to_owned(),
        },
    ];
    let transcript = serde_json::to_value(lines).expect("serializes");
    let (fetched, _) = fetched_with(connection, transcript).await;
    final_transcript::rebuild(connection, fetched.id, person())
        .await
        .expect("lines up");
    let done = meeting::record_done(connection, fetched.id)
        .await
        .expect("done");
    excerpts::refresh(connection, &done, None)
        .await
        .expect("indexes its transcript");
    done
}

/// Takes line `line` of `meeting`, one of [`segmented`]'s, into `project_id` as an excerpt, as a
/// person takes a range, and answers its record, which is indexed and on the project.
async fn excerpt_on(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    line: usize,
    project_id: Uuid,
) -> TimelineRecord {
    let words = meeting_word::standing(connection, meeting.id)
        .await
        .expect("reads");
    // Each line is five words.
    let range = Range {
        start_word_id: words[line * 5].id,
        end_word_id: words[line * 5 + 4].id,
    };
    let onto = [SegmentProject {
        project_id,
        confidence: 1.0,
    }];
    excerpts::assign(connection, meeting, range, &onto, person())
        .await
        .expect("takes it")
        .record
}

/// A topic `meeting` was cut into before excerpts that no word of it could anchor: a record that
/// names its meeting and no mark, still on `project_id`.
async fn unanchored_topic_on(
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

/// Whether the knowledge index holds meeting `meeting_id`'s whole transcript.
async fn is_transcript_indexed(connection: &mut AsyncPgConnection, meeting_id: Uuid) -> bool {
    let documents: i64 = knowledge_documents::table
        .filter(knowledge_documents::meeting_id.eq(meeting_id))
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
#[expect(
    clippy::too_many_lines,
    reason = "one meeting deleted and restored, with everything its excerpts leave and regain"
)]
async fn a_deleted_meetings_excerpts_leave_every_timeline_and_the_index_and_restoring_returns_them()
{
    let (_url, mut connection) = migrated_database().await;
    let project_id = launch_project(&mut connection).await;
    let launch = segmented(&mut connection).await;
    let excerpt = excerpt_on(&mut connection, &launch, 0, project_id).await;
    let topic_id = unanchored_topic_on(&mut connection, &launch, project_id).await;
    // An excerpt its person removed is on no project and out of the index already.
    let removed = excerpt_on(&mut connection, &launch, 1, project_id).await;
    excerpts::remove(
        &mut connection,
        &launch,
        removed.excerpt_mark_id.expect("an excerpt"),
        person(),
    )
    .await
    .expect("removes");
    let other = segmented(&mut connection).await;
    let kept = excerpt_on(&mut connection, &other, 0, project_id).await;
    assert!(is_indexed(&mut connection, excerpt.id).await);
    assert!(is_transcript_indexed(&mut connection, launch.id).await);

    let deleted = change(&mut connection, vec![launch.id], BulkChange::Delete {}).await;
    assert!(deleted.refused.is_empty(), "{:?}", deleted.refused);
    let [changed] = deleted.changed.as_slice() else {
        panic!("one meeting changed: {:?}", deleted.changed);
    };
    let deleted_at = changed.meeting.deleted_at.expect("it is deleted");
    let mut took: Vec<Uuid> = changed.records.iter().map(|record| record.id).collect();
    took.sort_unstable();
    let mut standing = vec![excerpt.id, topic_id];
    standing.sort_unstable();
    assert_eq!(
        took, standing,
        "its standing excerpt and its unanchored topic went with it, and only its own"
    );

    // It leaves every list but the deleted one, and its excerpts every reader.
    let listed: Vec<Uuid> = meetings_page(&mut connection, false)
        .await
        .iter()
        .map(|listed| listed.id)
        .collect();
    assert_eq!(listed, [other.id]);
    let deleted_list: Vec<Uuid> = meetings_page(&mut connection, true)
        .await
        .iter()
        .map(|listed| listed.id)
        .collect();
    assert_eq!(deleted_list, [launch.id]);
    assert_eq!(on_timeline(&mut connection, project_id).await, [kept.id]);
    assert!(!is_indexed(&mut connection, excerpt.id).await);
    assert!(!is_indexed(&mut connection, topic_id).await);
    assert!(is_indexed(&mut connection, kept.id).await);
    assert!(
        !is_transcript_indexed(&mut connection, launch.id).await,
        "its whole transcript is out of the index too"
    );
    assert!(is_transcript_indexed(&mut connection, other.id).await);
    assert!(matches!(
        timeline_record::find_owned(&mut connection, excerpt.id, TEST_PERSON_ID).await,
        Err(diesel::result::Error::NotFound)
    ));
    let read_by_manager = timeline_record::attributed_to(&mut connection, project_id, None, 50)
        .await
        .expect("reads");
    assert!(
        read_by_manager
            .iter()
            .all(|(record, _)| record.id != excerpt.id && record.id != topic_id),
        "the project's Project Manager no longer reads them"
    );
    let projects = timeline_record::projects_of_meetings(&mut connection, &[launch.id])
        .await
        .expect("reads");
    assert_eq!(
        projects.get(&launch.id),
        Some(&vec![project_id]),
        "its row among the deleted says where its excerpts were"
    );
    let walked = timeline_record::after_id(&mut connection, None, 100)
        .await
        .expect("walks");
    assert!(
        walked
            .iter()
            .all(|record| record.id != excerpt.id && record.id != topic_id),
        "the index's backfill walks past them, so they never come back on their own"
    );
    let text = MeetingText::load(&mut connection, launch.id)
        .await
        .expect("reads");
    excerpts::index_transcript(&mut connection, &changed.meeting, &text)
        .await
        .expect("walks it as the backfill does");
    assert!(
        !is_transcript_indexed(&mut connection, launch.id).await,
        "the backfill leaves its transcript out"
    );
    // Its attributions wait for it.
    assert_eq!(
        timeline_record::attributions_of(&mut connection, &[excerpt.id, topic_id])
            .await
            .expect("reads")
            .len(),
        2
    );
    let history = meeting_event::history(&mut connection, launch.id)
        .await
        .expect("history");
    let entry = history.last().expect("an entry");
    assert_eq!(entry.kind(), Some(EventKind::Deleted));
    assert_eq!(entry.created_by, TEST_PERSON_ID, "who deleted it is kept");
    assert_eq!(
        entry.data,
        json!({
            "excerpts": 2,
            "connectorId": null,
            "dismissedItems": [],
        })
    );

    let restored = change(&mut connection, vec![launch.id], BulkChange::Restore {}).await;
    assert!(restored.refused.is_empty(), "{:?}", restored.refused);
    assert_eq!(restored.changed[0].meeting.deleted_at, None);
    let mut on_project = on_timeline(&mut connection, project_id).await;
    on_project.sort_unstable();
    let mut back = vec![excerpt.id, topic_id, kept.id];
    back.sort_unstable();
    assert_eq!(
        on_project, back,
        "its excerpt and topic are on their project again"
    );
    assert!(is_indexed(&mut connection, excerpt.id).await);
    assert!(is_indexed(&mut connection, topic_id).await);
    assert!(
        !is_indexed(&mut connection, removed.id).await,
        "the excerpt its person removed stays removed"
    );
    assert!(is_transcript_indexed(&mut connection, launch.id).await);
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
        json!({
            "deletedAt": deleted_at,
            "excerpts": 2,
            "runRetried": false,
            "returnedItemIds": [],
        })
    );
}

// Restoring takes what deleting took, its owner or an admin, so the Deleted filter lists an admin
// every deleted meeting and anyone else their own, and a member who reads someone else's deleted
// meeting may neither delete nor restore it.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_deleted_list_holds_what_its_reader_may_restore() {
    let (_url, mut connection) = migrated_database().await;
    let (own, _) = stored(&mut connection).await;
    let (theirs, _) = stored(&mut connection).await;
    change(
        &mut connection,
        vec![own.id, theirs.id],
        BulkChange::Delete {},
    )
    .await;
    // Elysia's, and open to every member, so the test person reads it as a member.
    diesel::update(meetings::table.find(theirs.id))
        .set((
            meetings::created_by.eq(ELYSIA_USER_ID),
            meetings::visibility.eq("public"),
        ))
        .execute(&mut connection)
        .await
        .expect("handed to Elysia");

    let ids = |listed: Vec<Meeting>| -> Vec<Uuid> {
        let mut ids: Vec<Uuid> = listed.iter().map(|listed| listed.id).collect();
        ids.sort_unstable();
        ids
    };
    let member = Reader {
        id: TEST_PERSON_ID,
        reach: Reach::Public,
    };
    let admin = Reader {
        id: TEST_PERSON_ID,
        reach: Reach::Every,
    };
    let as_member = meeting::list_deleted(&mut connection, member, None, 50)
        .await
        .expect("lists");
    assert_eq!(
        ids(as_member),
        [own.id],
        "a member restores their own alone"
    );
    let as_admin = meeting::list_deleted(&mut connection, admin, None, 50)
        .await
        .expect("lists");
    let mut both = vec![own.id, theirs.id];
    both.sort_unstable();
    assert_eq!(ids(as_admin), both, "an admin restores every one");

    // The test person is a member here: Elysia's meeting refuses them as only its owner's or an
    // admin's to restore, while their own comes back.
    diesel::sql_query("UPDATE users SET role = 'member' WHERE id = $1")
        .bind::<diesel::sql_types::Uuid, _>(TEST_PERSON_ID)
        .execute(&mut connection)
        .await
        .expect("a member");
    let restored = change(
        &mut connection,
        vec![own.id, theirs.id],
        BulkChange::Restore {},
    )
    .await;
    let changed: Vec<Uuid> = restored
        .changed
        .iter()
        .map(|changed| changed.meeting.id)
        .collect();
    assert_eq!(changed, [own.id]);
    let refused: Vec<(Uuid, &str)> = restored
        .refused
        .iter()
        .map(|refusal| (refusal.meeting_id, refusal.code))
        .collect();
    assert_eq!(refused, [(theirs.id, "owner_or_admin")]);
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
        meetings_page(&mut connection, false).await.len(),
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

/// Stores a corrected copy of `meeting`'s transcript waiting for approval, and the inbox item
/// asking for it, as correction raises one. Answers the item's id.
async fn waiting_for_review(connection: &mut AsyncPgConnection, meeting: &Meeting) -> Uuid {
    meeting_correction::record(
        connection,
        NewMeetingCorrection {
            meeting_id: meeting.id,
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
        connection,
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
    meeting_correction::set_review_item(connection, meeting.id, review.record.id)
        .await
        .expect("names it");
    review.record.id
}

/// Raises the inbox item telling `meeting`'s person its correction kept it private, as
/// `visibility::decide` raises one, and names it on the meeting. Answers the item's id.
async fn kept_private(connection: &mut AsyncPgConnection, meeting: &Meeting) -> Uuid {
    let told = action_item::create(
        connection,
        NewActionItem {
            title: "Elysia kept a meeting private".to_owned(),
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
    meeting::set_visibility_item(connection, meeting.id, told.record.id)
        .await
        .expect("names it");
    told.record.id
}

async fn item_state(connection: &mut AsyncPgConnection, item_id: Uuid) -> ActionItemState {
    action_item::find(connection, item_id)
        .await
        .expect("found")
        .state
}

// A deleted meeting asks nothing of its person, so the items already asking them something about
// it are dismissed with it: to approve its corrected transcript, and whether to open it once its
// correction kept it private. Restoring asks again where each asked before, in the inbox, as the
// correction still waits.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn deleting_a_meeting_dismisses_the_items_asking_its_person_about_it() {
    let (_url, mut connection) = migrated_database().await;
    let (held, _) = stored(&mut connection).await;
    let review_id = waiting_for_review(&mut connection, &held).await;
    let kept_private_id = kept_private(&mut connection, &held).await;

    let deleted = change(&mut connection, vec![held.id], BulkChange::Delete {}).await;
    let mut dismissed: Vec<Uuid> = deleted.changed[0]
        .items
        .iter()
        .map(|item| item.record.id)
        .collect();
    dismissed.sort_unstable();
    let mut both = vec![review_id, kept_private_id];
    both.sort_unstable();
    assert_eq!(dismissed, both);
    for item_id in [review_id, kept_private_id] {
        assert_eq!(
            item_state(&mut connection, item_id).await,
            ActionItemState::Dismissed
        );
    }
    assert_eq!(
        deleted.changed[0].entry.data["dismissedItems"],
        json!([
            { "id": review_id, "from": "inbox" },
            { "id": kept_private_id, "from": "inbox" },
        ])
    );

    let restored = change(&mut connection, vec![held.id], BulkChange::Restore {}).await;
    assert_eq!(restored.changed[0].items.len(), 2);
    for item_id in [review_id, kept_private_id] {
        assert_eq!(
            item_state(&mut connection, item_id).await,
            ActionItemState::Inbox,
            "back in the inbox it was raised in, not opened"
        );
    }
    assert_eq!(
        restored.changed[0].entry.data["returnedItemIds"],
        json!([review_id, kept_private_id])
    );
    let correction = meeting_correction::find(&mut connection, held.id)
        .await
        .expect("reads")
        .expect("kept");
    assert_eq!(correction.approved_at, None);
}

// A review item its person accepted before deleting the meeting comes back accepted.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn restoring_returns_an_accepted_review_item_to_open() {
    let (_url, mut connection) = migrated_database().await;
    let (held, _) = stored(&mut connection).await;
    let review_id = waiting_for_review(&mut connection, &held).await;
    action_item::transition(
        &mut connection,
        review_id,
        crate::action_items::Transition::Accept,
        Actor::User(TEST_PERSON_ID),
        Utc::now(),
    )
    .await
    .expect("accepted");

    let deleted = change(&mut connection, vec![held.id], BulkChange::Delete {}).await;
    assert_eq!(
        deleted.changed[0].entry.data["dismissedItems"],
        json!([{ "id": review_id, "from": "open" }])
    );
    change(&mut connection, vec![held.id], BulkChange::Restore {}).await;
    assert_eq!(
        item_state(&mut connection, review_id).await,
        ActionItemState::Open
    );
}

// An item the person dealt with by hand while the meeting was deleted stays as they left it.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn restoring_leaves_a_review_item_the_person_resolved_meanwhile() {
    let (_url, mut connection) = migrated_database().await;
    let (held, _) = stored(&mut connection).await;
    let review_id = waiting_for_review(&mut connection, &held).await;
    change(&mut connection, vec![held.id], BulkChange::Delete {}).await;
    action_item::transition(
        &mut connection,
        review_id,
        crate::action_items::Transition::Reopen,
        Actor::User(TEST_PERSON_ID),
        Utc::now(),
    )
    .await
    .expect("reopened by hand");
    action_item::transition(
        &mut connection,
        review_id,
        crate::action_items::Transition::Resolve,
        Actor::User(TEST_PERSON_ID),
        Utc::now(),
    )
    .await
    .expect("resolved by hand");

    let restored = change(&mut connection, vec![held.id], BulkChange::Restore {}).await;
    assert!(restored.changed[0].items.is_empty());
    assert_eq!(
        item_state(&mut connection, review_id).await,
        ActionItemState::Resolved
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn deleting_a_meeting_with_no_review_dismisses_nothing() {
    let (_url, mut connection) = migrated_database().await;
    let (held, _) = stored(&mut connection).await;

    let deleted = change(&mut connection, vec![held.id], BulkChange::Delete {}).await;
    assert!(deleted.changed[0].items.is_empty());
    assert_eq!(deleted.changed[0].entry.data["dismissedItems"], json!([]));
    let restored = change(&mut connection, vec![held.id], BulkChange::Restore {}).await;
    assert!(restored.changed[0].items.is_empty());
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
        let listed_ids: Vec<Uuid> = meetings_page(&mut connection, false)
            .await
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
    let stays = meetings_page(&mut connection, false).await;
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
    let deleted = meetings_page(&mut connection, true).await;
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
    let left: Vec<Uuid> = meetings_page(&mut connection, false)
        .await
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
    let live = meetings_page(&mut connection, false).await;
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
