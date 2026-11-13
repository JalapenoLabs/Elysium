// Copyright © 2026 Jalapeno Labs

//! What agents wrote from a private meeting, through every route that reads it (#278): a Project
//! Manager's turn, its record, and the batch it proposed, a session's turn, its events, and its
//! summary's record, and Elysia's turn, each read only by whoever may read the meeting, by listing
//! (the project's timeline, the Project Manager tab, the work queue, Elysia's activity, the
//! session's conversation), by opening (a record, a turn, a batch, a document), and by searching
//! the knowledge base. A record written from two private meetings
//! needs both. Sharing the meeting opens what was held back, so does making it public, and making
//! it private again closes it at the next request, with nothing written again; an admin reads it
//! all.
//!
//! What each reader is answered is held to what `docs/knowledge.md`, Who sees what, What agents
//! write from a meeting, says, written out here apart from the code that decides it. `#[ignore]`d
//! so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs it.

use std::collections::HashSet;

use axum::http::StatusCode;
use chrono::Utc;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::models::coding_session::{self, NewCodingSession};
use crate::models::elysia::{self, NewTurn};
use crate::models::meeting::{self, Visibility};
use crate::models::meeting_reads::{self, Turn};
use crate::models::meeting_share;
use crate::models::project::{self, NewProject};
use crate::models::project_manager::{self, Finished};
use crate::models::promise::{self, NewPromise};
use crate::models::session_event::{self, StoredEvent};
use crate::models::session_turn::{self, NewSessionTurn};
use crate::models::timeline_record::{self, NewTimelineRecord};
use crate::models::turn_summary;
use crate::models::user::PROJECT_MANAGER_USER_ID;
use crate::models::work_batch::{self, NewWorkBatch, Priority};
use crate::project_managers::timeline as manager_timeline;
use crate::routes::v1::knowledge::tests::{coworker, document_of_record, get, set_role};
use crate::runner::{RunnerEvent, TurnOutcome, TurnStatus};
use crate::sessions::timeline as session_timeline;
use crate::state::AppState;
use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database, person_rooted};

/// What agents wrote, and what was written from no meeting beside it.
struct Written {
    project_id: Uuid,
    /// A Project Manager's turn that read the private meeting, and its record and document.
    manager_turn: Uuid,
    manager_record: Uuid,
    manager_document: Uuid,
    /// A Project Manager's turn that read the private meeting and another one, and its record and
    /// document.
    both_turn: Uuid,
    both_record: Uuid,
    both_document: Uuid,
    /// The batch the Project Manager's turn that read the private meeting proposed, and one its
    /// turn that read no meeting proposed.
    batch: Uuid,
    open_batch: Uuid,
    /// A session's turn that read the private meeting, which reported an event, and its summary's
    /// record and document.
    session_id: i64,
    session_turn: Uuid,
    summary_record: Uuid,
    summary_document: Uuid,
    /// One of Elysia's turns that answered in the private meeting's thread.
    elysia_turn: Uuid,
    /// A promise made in the private meeting, which the Project Manager keeps.
    promise: Uuid,
    /// A Project Manager's turn that read no meeting, and its record and document.
    open_turn: Uuid,
    open_record: Uuid,
    open_document: Uuid,
}

/// A meeting of `owner`'s, read as `visibility` says.
async fn meeting_of(
    connection: &mut AsyncPgConnection,
    owner: Uuid,
    visibility: Visibility,
) -> Uuid {
    let (found, _capture) = meeting::tests::stored(connection).await;
    diesel::sql_query(format!(
        "UPDATE meetings SET created_by = '{owner}', visibility = '{}' WHERE id = '{}'",
        visibility.as_str(),
        found.id
    ))
    .execute(connection)
    .await
    .expect("the meeting is set up");
    found.id
}

/// A finished turn of `project_id`'s Project Manager that read `read`, summarized by `summary`,
/// with its record on the project's timeline: its turn and its record.
async fn finished_manager_turn(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    read: &[Uuid],
    summary: &str,
) -> (Uuid, Uuid) {
    project_manager::wake_with_message(connection, project_id, "Plan it.", TEST_PERSON_ID)
        .await
        .expect("woken");
    let (turn, _wakes) = project_manager::start_turn(connection, project_id, 10)
        .await
        .expect("starts")
        .expect("a turn");
    meeting_reads::keep(connection, Turn::ProjectManager(turn.id), read)
        .await
        .expect("kept");
    let record =
        manager_timeline::record_turn(connection, project_id, turn.id, summary, None, Utc::now())
            .await
            .expect("recorded");
    project_manager::finish_turn(
        connection,
        turn.id,
        Finished::Succeeded {
            summary: Some(summary),
            reply: None,
            timeline_record_id: Some(record.id),
        },
        "Called manager_record",
    )
    .await
    .expect("finished");
    (turn.id, record.id)
}

/// A batch `turn_id`, a turn of `project_id`'s Project Manager, proposed, waiting for a person.
async fn proposed_by(connection: &mut AsyncPgConnection, project_id: Uuid, turn_id: Uuid) -> Uuid {
    let batch = NewWorkBatch {
        id: Uuid::now_v7(),
        project_id,
        title: "Move the team".to_owned(),
        goal: "Do the work.".to_owned(),
        repositories: json!([]),
        priority: Priority::Normal,
        scheduled_for: None,
        turn_id: Some(turn_id),
        source_record_ids: json!([]),
        action_item_ids: json!([]),
        serves: None,
        source_message_ids: json!([]),
        lineage: person_rooted(),
        created_by: PROJECT_MANAGER_USER_ID,
    };
    work_batch::submit(connection, batch)
        .await
        .expect("submitted")
        .batch
        .id
}

/// A turn of a session of `project_id` that read `meeting`, reported an event, and was summarized:
/// its session, its turn, and its summary's record on the project's timeline.
async fn summarized_session_turn(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    meeting: Uuid,
) -> (i64, Uuid, Uuid) {
    let id = coding_session::reserve_id(connection)
        .await
        .expect("a number");
    let session = coding_session::create(
        connection,
        &NewCodingSession {
            id,
            project_id: Some(project_id),
            title: "Reorg notes".to_owned(),
            github_credential_id: None,
            action_item_id: None,
            studio_item_id: None,
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("a session");
    let turn = session_turn::queue(
        connection,
        &NewSessionTurn {
            id: Uuid::now_v7(),
            session_id: session.id,
            prompt: "What did the one-to-one decide?",
            system_prompt_append: None,
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("queued")
    .id;
    meeting_reads::keep(
        connection,
        Turn::Session {
            session_id: session.id,
            turn_id: turn,
        },
        &[meeting],
    )
    .await
    .expect("kept");
    session_event::record(
        connection,
        session.id,
        &StoredEvent {
            sequence: 1,
            turn_id: Some(turn),
            occurred_at: Utc::now(),
            event: RunnerEvent::Finished(TurnOutcome::without_result(TurnStatus::Completed, "")),
        },
    )
    .await
    .expect("recorded");
    let summary = turn_summary::record(
        connection,
        session.id,
        turn,
        "Read why the quokka team moves.",
    )
    .await
    .expect("summarized");
    let record = session_timeline::record(connection, &summary, &session)
        .await
        .expect("recorded")
        .expect("on its project")
        .id;
    (session.id, turn, record)
}

/// One of Elysia's turns that answered in `meeting`'s thread, which she read, finished.
async fn answered_thread_turn(connection: &mut AsyncPgConnection, meeting: Uuid) -> Uuid {
    let turn = elysia::insert_turn(
        connection,
        NewTurn {
            id: Uuid::now_v7(),
            trigger_kind: "meeting-comment".to_owned(),
            trigger_detail: json!({ "meetingId": meeting }),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("queued")
    .id;
    elysia::start_turn(connection, turn)
        .await
        .expect("starts")
        .expect("started");
    meeting_reads::keep(connection, Turn::Elysia(turn), &[meeting])
        .await
        .expect("kept");
    elysia::finish_turn(
        connection,
        turn,
        Ok(Some("Answered that the quokka team moves in May.")),
    )
    .await
    .expect("finished");
    turn
}

/// A promise made in an excerpt of `meeting`, on `project_id`, as segmenting records one.
async fn promised_in(connection: &mut AsyncPgConnection, project_id: Uuid, meeting: Uuid) -> Uuid {
    let excerpt = timeline_record::insert_new(
        connection,
        NewTimelineRecord {
            id: Uuid::now_v7(),
            source_kind: "meeting".to_owned(),
            connector_id: None,
            external_id: None,
            occurred_at: Utc::now(),
            actor: "Grace".to_owned(),
            // None of the words searched for, so what a search finds is what agents wrote alone.
            title: "The team's move".to_owned(),
            body: "[0:05] Grace: I'll tell the team once the move is live.".to_owned(),
            url: None,
            media: None,
            metadata: json!({ "meetingId": meeting }),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("stored")
    .expect("new");
    promise::record(
        connection,
        &[NewPromise {
            project_id,
            timeline_record_id: excerpt.id,
            recipient: "the team".to_owned(),
            summary: "Tell the team once the move is live".to_owned(),
        }],
    )
    .await
    .expect("promised")
    .remove(0)
    .id
}

/// Everything agents wrote, `private` and `other` being two meetings the test person does not own.
async fn written(connection: &mut AsyncPgConnection, private: Uuid, other: Uuid) -> Written {
    let project_id = project::create(
        connection,
        &NewProject {
            created_by: TEST_PERSON_ID,
            name: "Reorg".to_owned(),
            description: String::new(),
        },
    )
    .await
    .expect("a project")
    .id;
    let (manager_turn, manager_record) = finished_manager_turn(
        connection,
        project_id,
        &[private],
        "Planned the quokka team's move",
    )
    .await;
    let (both_turn, both_record) = finished_manager_turn(
        connection,
        project_id,
        &[private, other],
        "Planned the wombat budget",
    )
    .await;
    let (open_turn, open_record) =
        finished_manager_turn(connection, project_id, &[], "Planned the pelican launch").await;
    let batch = proposed_by(connection, project_id, manager_turn).await;
    let open_batch = proposed_by(connection, project_id, open_turn).await;
    let (session_id, session_turn, summary_record) =
        summarized_session_turn(connection, project_id, private).await;
    let elysia_turn = answered_thread_turn(connection, private).await;
    let promise = promised_in(connection, project_id, private).await;

    Written {
        project_id,
        manager_turn,
        manager_record,
        manager_document: document_of_record(connection, manager_record).await,
        both_turn,
        both_record,
        both_document: document_of_record(connection, both_record).await,
        batch,
        open_batch,
        session_id,
        session_turn,
        summary_record,
        summary_document: document_of_record(connection, summary_record).await,
        elysia_turn,
        promise,
        open_turn,
        open_record,
        open_document: document_of_record(connection, open_record).await,
    }
}

/// The ids under `key` of each object in `list`.
fn ids(list: &Value, key: &str) -> HashSet<String> {
    list.as_array()
        .expect("a list")
        .iter()
        .map(|entry| entry[key].as_str().expect("an id").to_owned())
        .collect()
}

/// What the test person reads of what was written, through every route that reads it.
#[derive(Debug, PartialEq, Eq)]
struct Seen {
    /// The records the project's timeline lists, and the total it counts.
    timeline: HashSet<String>,
    timeline_total: i64,
    /// The records opened by id.
    records: HashSet<String>,
    /// The documents a search finds, and those opened by id.
    found: HashSet<String>,
    documents: HashSet<String>,
    /// The turns the Project Manager tab lists, and those opened by id.
    manager_turns: HashSet<String>,
    manager_opened: HashSet<String>,
    /// Elysia's turns listed, and those opened by id.
    elysia_turns: HashSet<String>,
    elysia_opened: HashSet<String>,
    /// The promises the Project Manager tab lists.
    promises: HashSet<String>,
    /// The batches the Project Manager tab lists, latest and proposed, those the work queue lists
    /// proposed, and those opened by id, with their worker's log.
    manager_batches: HashSet<String>,
    manager_proposed: HashSet<String>,
    queued: HashSet<String>,
    batches_opened: HashSet<String>,
    logs_opened: HashSet<String>,
    /// The turns the session's conversation has events and summaries of, and events of as it
    /// catches up from a sequence.
    session_events: HashSet<String>,
    summaries: HashSet<String>,
    caught_up: HashSet<String>,
}

/// The ids among `ids` that `GET path_of(id)` answers `200`, failing on anything but `200` or `404`.
async fn opened(
    state: &AppState,
    candidates: &[Uuid],
    path_of: impl Fn(Uuid) -> String,
) -> HashSet<String> {
    let mut opened = HashSet::new();
    for id in candidates {
        let (status, body) = get(state, &path_of(*id)).await;
        match status {
            StatusCode::OK => {
                opened.insert(id.to_string());
            }
            StatusCode::NOT_FOUND => {}
            other => panic!("{} answered {other}: {body}", path_of(*id)),
        }
    }
    opened
}

async fn seen(state: &AppState, written: &Written) -> Seen {
    let project = written.project_id;
    let (status, timeline) = get(state, &format!("/api/v1/timeline?projectId={project}")).await;
    assert_eq!(status, StatusCode::OK, "{timeline}");
    let records = [
        written.manager_record,
        written.both_record,
        written.summary_record,
        written.open_record,
    ];
    // One search for every word written, so the test spends one of the person's searches a look.
    let (status, answer) = get(
        state,
        "/api/v1/knowledge/search?q=quokka%20or%20wombat%20or%20pelican",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let found: HashSet<String> = answer["results"]
        .as_array()
        .expect("results")
        .iter()
        .map(|result| {
            result["citation"]["documentId"]
                .as_str()
                .expect("an id")
                .to_owned()
        })
        .collect();
    let documents = [
        written.manager_document,
        written.both_document,
        written.summary_document,
        written.open_document,
    ];
    let (status, manager) = get(
        state,
        &format!("/api/v1/projects/{project}/project-manager"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{manager}");
    let manager_turns = [written.manager_turn, written.both_turn, written.open_turn];
    let (status, activity) = get(state, "/api/v1/elysia/turns").await;
    assert_eq!(status, StatusCode::OK, "{activity}");
    let session = written.session_id;
    let (status, history) = get(state, &format!("/api/v1/coding-sessions/{session}/events")).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    let (status, tail) = get(
        state,
        &format!("/api/v1/coding-sessions/{session}/events?afterSequence=0"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{tail}");
    let (status, queue) = get(state, "/api/v1/work-queue").await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    let batches = [written.batch, written.open_batch];

    Seen {
        timeline: ids(&timeline["records"], "id"),
        timeline_total: timeline["total"].as_i64().expect("a total"),
        records: opened(state, &records, |id| format!("/api/v1/timeline/{id}")).await,
        found,
        documents: opened(state, &documents, |id| {
            format!("/api/v1/knowledge/documents/{id}")
        })
        .await,
        manager_turns: ids(&manager["projectManager"]["turns"], "id"),
        manager_opened: opened(state, &manager_turns, |id| {
            format!("/api/v1/projects/{project}/project-manager/turns/{id}")
        })
        .await,
        elysia_turns: ids(&activity["turns"], "id"),
        promises: ids(&manager["projectManager"]["promises"], "id"),
        elysia_opened: opened(state, &[written.elysia_turn], |id| {
            format!("/api/v1/elysia/turns/{id}")
        })
        .await,
        manager_batches: ids(&manager["projectManager"]["batches"], "id"),
        manager_proposed: ids(&manager["projectManager"]["proposed"], "id"),
        queued: ids(&queue["workQueue"]["proposed"], "id"),
        batches_opened: opened(state, &batches, |id| format!("/api/v1/work-batches/{id}")).await,
        logs_opened: opened(state, &batches, |id| {
            format!("/api/v1/work-batches/{id}/worker-log")
        })
        .await,
        session_events: ids(&history["events"], "turnId"),
        summaries: ids(&history["summaries"], "turnId"),
        caught_up: ids(&tail["events"], "turnId"),
    }
}

/// What a reader is answered, as `docs/knowledge.md` says it, apart from the code that decides it:
/// with `reads_private`, the private meeting's writing; with `reads_both`, also what was written
/// from it and the other meeting together. What was written from no meeting is always there.
fn expected(written: &Written, reads_private: bool, reads_both: bool) -> Seen {
    let set = |ids: &[(Uuid, bool)]| -> HashSet<String> {
        ids.iter()
            .filter(|(_id, shown)| *shown)
            .map(|(id, _shown)| id.to_string())
            .collect()
    };
    let records = set(&[
        (written.manager_record, reads_private),
        (written.both_record, reads_both),
        (written.summary_record, reads_private),
        (written.open_record, true),
    ]);
    let documents = set(&[
        (written.manager_document, reads_private),
        (written.both_document, reads_both),
        (written.summary_document, reads_private),
        (written.open_document, true),
    ]);
    let manager_turns = set(&[
        (written.manager_turn, reads_private),
        (written.both_turn, reads_both),
        (written.open_turn, true),
    ]);
    let elysia_turns = set(&[(written.elysia_turn, reads_private)]);
    let batches = set(&[(written.batch, reads_private), (written.open_batch, true)]);
    Seen {
        timeline_total: i64::try_from(records.len()).expect("a few records"),
        timeline: records.clone(),
        records,
        found: documents.clone(),
        documents,
        manager_opened: manager_turns.clone(),
        manager_turns,
        elysia_opened: elysia_turns.clone(),
        elysia_turns,
        promises: set(&[(written.promise, reads_private)]),
        manager_batches: batches.clone(),
        manager_proposed: batches.clone(),
        queued: batches.clone(),
        batches_opened: batches.clone(),
        logs_opened: batches,
        session_events: set(&[(written.session_turn, reads_private)]),
        summaries: set(&[(written.session_turn, reads_private)]),
        caught_up: set(&[(written.session_turn, reads_private)]),
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn what_agents_wrote_from_a_private_meeting_is_read_only_by_its_readers() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let grace = coworker(&mut connection, "Grace Hopper").await;
    let private = meeting_of(&mut connection, grace, Visibility::Private).await;
    let other = meeting_of(&mut connection, grace, Visibility::Private).await;
    let written = written(&mut connection, private, other).await;
    let expect =
        |reads_private: bool, reads_both: bool| expected(&written, reads_private, reads_both);

    // A member nobody shared it with reads none of it, but what was written from no meeting.
    set_role(&mut connection, "member").await;
    assert_eq!(seen(&state, &written).await, expect(false, false));

    // Shared with them, they read what was written from it, but not what also came from a
    // meeting they may not read.
    meeting_share::add(&mut connection, private, TEST_PERSON_ID, grace)
        .await
        .expect("shared");
    assert_eq!(seen(&state, &written).await, expect(true, false));
    meeting_share::remove(&mut connection, private, TEST_PERSON_ID)
        .await
        .expect("unshared");
    assert_eq!(seen(&state, &written).await, expect(false, false));

    // Opened to everyone, every member reads it; made private again, it closes at once, with
    // nothing written again.
    meeting::set_visibility(&mut connection, private, Visibility::Public)
        .await
        .expect("opened");
    assert_eq!(seen(&state, &written).await, expect(true, false));
    meeting::set_visibility(&mut connection, other, Visibility::Public)
        .await
        .expect("opened");
    assert_eq!(seen(&state, &written).await, expect(true, true));
    meeting::set_visibility(&mut connection, private, Visibility::Private)
        .await
        .expect("closed");
    assert_eq!(seen(&state, &written).await, expect(false, false));

    // An admin reads every meeting, and so all that was written from them.
    set_role(&mut connection, "admin").await;
    assert_eq!(seen(&state, &written).await, expect(true, true));
}
