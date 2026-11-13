// Copyright © 2026 Jalapeno Labs

//! Who reads and changes a meeting, route by route (#246): every meeting route asked by a member,
//! an admin, and a guest, each as the meeting's owner, someone it is shared with, and someone else,
//! of a private meeting and a public one. Each answer is held to what `docs/meeting-review.md`,
//! Who sees a meeting, says, written out here apart from the code that decides it
//! ([`expected`]): a meeting someone may not read is `404`, as one that does not exist is, and one
//! they read but may not change in that way names what it takes.
//!
//! A request allowed through is sent so it then fails its own checks (an empty edit, the meeting's
//! own visibility again, an offset out of range, a pair of meetings decided already, an excerpt on
//! no project, a backwards range, a thread removed as a mark, a frame sent as no form, a file past
//! the largest a meeting takes), so the matrix can run every cell
//! against one meeting without changing what it decides; a retry is the one that acts, so its run
//! is failed again before each. They are `#[ignore]`d so `cargo test` stays hermetic;
//! `api/scripts/verify-migrations.sh` runs them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use super::tests::{insert_grace, keep_recording, thread_item_for};
use crate::action_items::Actor;
use crate::meetings::{visibility, workflow};
use crate::models::action_item::{self, ActionItemPriority, ActionItemState, NewActionItem, Owner};
use crate::models::agent::Author;
use crate::models::meeting_attachment::{self, NewAttachment, StoredFrame};
use crate::models::meeting_capture;
use crate::models::meeting_correction::{self, NewMeetingCorrection};
use crate::models::meeting_duplicate::{self, NewMeetingDuplicate};
use crate::models::meeting_mark::{self, MarkKind, NewMark, Range};
use crate::models::meeting_task::{self, OwedBy, TaskReading};
use crate::models::meeting_word::tests::{built_meeting, word};
use crate::models::step_job::{self, StepOutcome};
use crate::models::user::ELYSIA_USER_ID;
use crate::realtime::EventBus;
use crate::routes;
use crate::state::AppState;
use crate::storage::tests::fake_storage;
use crate::test_support::{
    TEST_PERSON_ID, app_state_with_storage, migrated_database, signed_in_cookie,
};
use crate::workflows;

/// The signed-in test person's role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Member,
    Admin,
    Guest,
}

impl Role {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Member => "member",
            Self::Admin => "admin",
            Self::Guest => "guest",
        }
    }
}

/// Whose the meeting is, to the test person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Relation {
    Owner,
    SharedWith,
    Other,
}

/// What a request needs of the meeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Needs {
    Read,
    Edit,
    Manage,
    Own,
}

/// What a request should answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expected {
    /// Through to the route's own work: neither `404` nor `403`.
    Allowed,
    NotFound,
    /// `403` with this code.
    Refused(&'static str),
}

/// Who sees a meeting, as `docs/meeting-review.md` says it, apart from the code that decides it:
/// every member reads a public meeting, its owner and the people it is shared with read a private
/// one, admins read every meeting, and a guest reads only what is theirs or shared with them. A
/// company member who reads it edits it, and so does its owner; its owner and admins manage it;
/// only its owner runs its processing.
fn expected(needs: Needs, role: Role, relation: Relation, public: bool) -> Expected {
    let reads = match (role, relation) {
        (_, Relation::Owner | Relation::SharedWith) | (Role::Admin, Relation::Other) => true,
        (Role::Member, Relation::Other) => public,
        (Role::Guest, Relation::Other) => false,
    };
    if !reads {
        return Expected::NotFound;
    }
    let owns = relation == Relation::Owner;
    let allowed = match needs {
        Needs::Read => true,
        Needs::Edit => owns || role != Role::Guest,
        Needs::Manage => owns || role == Role::Admin,
        Needs::Own => owns,
    };
    if allowed {
        return Expected::Allowed;
    }
    Expected::Refused(match needs {
        Needs::Read | Needs::Edit => "read_only",
        Needs::Manage => "owner_or_admin",
        Needs::Own => "owner_only",
    })
}

/// One request of the matrix.
struct Asked {
    method: Method,
    path: String,
    body: Option<Value>,
    range: Option<&'static str>,
    needs: Needs,
}

impl Asked {
    fn new(method: Method, path: String, needs: Needs) -> Self {
        Self {
            method,
            path,
            body: None,
            range: None,
            needs,
        }
    }

    fn with_body(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }
}

/// Sends `method` to `path` as the signed-in test person, with `range` when given, and answers
/// the status and the body, as JSON when it is.
async fn ask(
    state: &AppState,
    method: Method,
    path: &str,
    body: Option<&Value>,
    range: Option<&str>,
) -> (StatusCode, Vec<u8>) {
    let cookie = signed_in_cookie(state).await;
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::COOKIE, cookie)
        .header(header::ORIGIN, "http://localhost:4000")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(range) = range {
        request = request.header(header::RANGE, range);
    }
    let response = routes::router(state)
        .with_state(state.clone())
        .oneshot(
            request
                .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
                .expect("a request builds"),
        )
        .await
        .expect("the router answers");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("the body reads");
    (status, bytes.to_vec())
}

async fn execute(connection: &mut AsyncPgConnection, statement: String) {
    diesel::sql_query(statement)
        .execute(connection)
        .await
        .expect("the statement runs");
}

/// Fails whatever step of a run is queued, as the engine records a step that failed for good, so
/// the meeting's run is failed for a retry to take.
async fn fail_what_is_queued(connection: &mut AsyncPgConnection) {
    let at = chrono::Utc::now();
    let claimed = step_job::claim(connection, at, 1).await.expect("claims");
    for claim in &claimed.claims {
        step_job::finish(
            connection,
            claim,
            StepOutcome::Failed {
                reason: "Fireflies refused the key".to_owned(),
                retryable: false,
            },
            at,
        )
        .await
        .expect("records")
        .expect("the claim holds");
    }
}

/// Makes each of `meeting_ids` what this cell of the matrix needs: the test person of `role`,
/// related to it as `relation`, and it public or private. Its other owner is `someone`.
async fn arrange(
    connection: &mut AsyncPgConnection,
    meeting_ids: &[Uuid],
    someone: Uuid,
    (role, relation, public): (Role, Relation, bool),
) {
    let owner = if relation == Relation::Owner {
        TEST_PERSON_ID
    } else {
        someone
    };
    let visibility = if public { "public" } else { "private" };
    execute(
        connection,
        format!(
            "UPDATE users SET role = '{}' WHERE id = '{TEST_PERSON_ID}'",
            role.as_str()
        ),
    )
    .await;
    execute(
        connection,
        format!("DELETE FROM meeting_shares WHERE person_id = '{TEST_PERSON_ID}'"),
    )
    .await;
    for meeting_id in meeting_ids {
        execute(
            connection,
            format!(
                "UPDATE meetings SET created_by = '{owner}', visibility = '{visibility}' \
                 WHERE id = '{meeting_id}'"
            ),
        )
        .await;
        if relation == Relation::SharedWith {
            execute(
                connection,
                format!(
                    "INSERT INTO meeting_shares (meeting_id, person_id, created_by) \
                     VALUES ('{meeting_id}', '{TEST_PERSON_ID}', '{someone}')"
                ),
            )
            .await;
        }
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one meeting set up with every part a route reads, then every route in every cell"
)]
async fn every_meeting_route_answers_exactly_who_may_read_or_change_it() {
    let (url, mut connection) = migrated_database().await;
    let (storage, files) = fake_storage().await;
    let state = app_state_with_storage(&url, storage).await;
    let grace = insert_grace(&mut connection).await;

    // A meeting with every part a route reads: words and an edit of them, a thread raising an
    // inbox item, a share whose undo a manager may take, a recording, and a corrected copy.
    let (capture, words) = built_meeting(&mut connection, &[("Ada", "Sheriff ships Friday")]).await;
    let meeting_id = capture.meeting_id;
    keep_recording(&state, &mut connection, &files, capture.id, b"0123456789").await;
    let (status, edited) = ask(
        &state,
        Method::POST,
        &format!("/api/v1/meetings/{meeting_id}/edits"),
        Some(&json!({
            "startWordId": word(&words, "Friday").id,
            "endWordId": word(&words, "Friday").id,
            "text": "Monday",
        })),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&edited)
    );
    let edited: Value = serde_json::from_slice(&edited).expect("JSON");
    let edit_entry = edited["entry"]["id"].as_str().expect("an entry").to_owned();
    // Shared with Grace before she is mentioned, since only those who may read a meeting are.
    let shared = visibility::share(
        &mut connection,
        meeting_id,
        grace,
        crate::models::agent::Author {
            user_id: TEST_PERSON_ID,
            agent: None,
            at: chrono::Utc::now(),
        },
    )
    .await
    .expect("shared with Grace");
    let sheriff = word(&words, "Sheriff");
    let ships = word(&words, "ships");
    let (status, started) = ask(
        &state,
        Method::POST,
        &format!("/api/v1/meetings/{meeting_id}/threads"),
        Some(&json!({
            "startWordId": sheriff.id,
            "endWordId": sheriff.id,
            "body": "Grace, is this Sharath?",
            "mentions": [grace],
        })),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&started)
    );
    let started: Value = serde_json::from_slice(&started).expect("JSON");
    let thread_id = started["comment"]["threadId"]
        .as_str()
        .expect("a thread")
        .to_owned();
    let item_id = thread_item_for(&mut connection, grace).await;
    meeting_correction::record(
        &mut connection,
        NewMeetingCorrection {
            meeting_id,
            transcript: json!([]),
            transcript_hash: "b".repeat(64),
            based_on: "source".to_owned(),
            source_transcript_hash: "a".repeat(64),
            whisper_transcript_hash: None,
            changed_lines: 0,
            fixes: json!([]),
        },
    )
    .await
    .expect("records")
    .expect("the first copy");
    // Its run, failed, for a retry to take.
    workflows::start(
        &mut connection,
        &EventBus::in_process(),
        workflow::run_for(meeting_id),
    )
    .await
    .expect("the run starts");
    // And a meeting it looks like, the same person's, decided already, asked about by an item.
    let (same_as, _) = built_meeting(&mut connection, &[("Ada", "Sheriff ships Friday")]).await;
    let duplicate_item = action_item::create(
        &mut connection,
        NewActionItem {
            title: "Same meeting as Launch sync?".to_owned(),
            notes: String::new(),
            state: ActionItemState::Inbox,
            priority: ActionItemPriority::Normal,
            due_at: None,
            owner: Owner::User,
            project_ids: Vec::new(),
            initiative_ids: Vec::new(),
        },
        Actor::System,
        chrono::Utc::now(),
    )
    .await
    .expect("an item");
    let pair = meeting_duplicate::insert_new(
        &mut connection,
        NewMeetingDuplicate {
            id: Uuid::now_v7(),
            meeting_id,
            same_as_id: same_as.meeting_id,
            action_item_id: Some(duplicate_item.record.id),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("records")
    .expect("a new pair");
    execute(
        &mut connection,
        format!(
            "UPDATE meeting_duplicates SET decision = 'not_the_same', decided_by = '{grace}', \
             decided_at = now() WHERE id = '{}'",
            pair.id
        ),
    )
    .await;

    // A task Elysia heard over its words, and the item made of it, which the item's page asks
    // after.
    let task_mark = meeting_mark::insert(
        &mut connection,
        NewMark::new(
            meeting_id,
            MarkKind::Task,
            Range {
                start_word_id: ships.id,
                end_word_id: ships.id,
            },
            json!({}),
            Author {
                user_id: ELYSIA_USER_ID,
                agent: None,
                at: chrono::Utc::now(),
            },
        ),
    )
    .await
    .expect("a task's mark");
    let task = TaskReading {
        title: "Ship it on Friday".to_owned(),
        owed_by: OwedBy::Nobody,
        project_id: None,
    };
    meeting_task::insert(
        &mut connection,
        task_mark.id,
        meeting_id,
        &task,
        ELYSIA_USER_ID,
    )
    .await
    .expect("a task");
    let task_item = action_item::create(
        &mut connection,
        NewActionItem {
            title: task.title.clone(),
            notes: String::new(),
            state: ActionItemState::Inbox,
            priority: ActionItemPriority::Normal,
            due_at: None,
            owner: Owner::User,
            project_ids: Vec::new(),
            initiative_ids: Vec::new(),
        },
        Actor::Elysia,
        chrono::Utc::now(),
    )
    .await
    .expect("an item")
    .record
    .id;
    meeting_task::record_item(
        &mut connection,
        task_mark.id,
        task_item,
        Some(TEST_PERSON_ID),
        ELYSIA_USER_ID,
    )
    .await
    .expect("its item is recorded");

    // A frame of its recording, which the first cell allowed to take it off does; the rest find it
    // off already, which is still their own refusal.
    let recording_location = meeting_capture::find(&mut connection, capture.id)
        .await
        .expect("the capture")
        .storage_location_id
        .expect("its recording is kept");
    let frame = meeting_attachment::insert(
        &mut connection,
        NewAttachment::frame(
            StoredFrame {
                id: Uuid::now_v7(),
                meeting_id,
                storage_location_id: recording_location,
                storage_path: format!("meeting-attachments/{meeting_id}/frames/still.webp"),
                file_name: "Frame at 0m01s.webp".to_owned(),
                size_bytes: 4,
                capture_id: capture.id,
                capture_ms: 1_000,
                caption: "The launch slide".to_owned(),
                fingerprint: vec![0; 576],
            },
            Author {
                user_id: TEST_PERSON_ID,
                agent: None,
                at: chrono::Utc::now(),
            },
        ),
    )
    .await
    .expect("a frame");

    let meeting = format!("/api/v1/meetings/{meeting_id}");
    let captured = format!("{meeting}/captures/{}", capture.id);
    let threads = format!("{meeting}/threads");
    let thread = format!("{threads}/{thread_id}");
    let mut media = Asked::new(Method::GET, format!("{captured}/media"), Needs::Read);
    media.range = Some("bytes=2-5");
    let requests = [
        Asked::new(Method::GET, meeting.clone(), Needs::Read),
        Asked::new(Method::GET, format!("{meeting}/words"), Needs::Read),
        Asked::new(Method::GET, format!("{captured}/transcript"), Needs::Read),
        media,
        Asked::new(Method::GET, format!("{meeting}/history"), Needs::Read),
        Asked::new(Method::GET, threads.clone(), Needs::Read),
        Asked::new(Method::GET, format!("{meeting}/tasks"), Needs::Read),
        Asked::new(Method::POST, format!("{meeting}/edits"), Needs::Edit).with_body(json!({
            "startWordId": sheriff.id,
            "endWordId": sheriff.id,
            "text": " ",
        })),
        Asked::new(
            Method::POST,
            format!("{meeting}/history/{edit_entry}/undo"),
            Needs::Edit,
        ),
        Asked::new(Method::POST, threads.clone(), Needs::Edit).with_body(json!({
            "startWordId": sheriff.id,
            "endWordId": sheriff.id,
            "body": " ",
        })),
        Asked::new(Method::POST, format!("{thread}/comments"), Needs::Edit)
            .with_body(json!({ "body": " " })),
        Asked::new(Method::POST, format!("{thread}/resolved"), Needs::Edit),
        Asked::new(Method::DELETE, format!("{thread}/resolved"), Needs::Edit),
        Asked::new(Method::DELETE, thread.clone(), Needs::Edit),
        // Into no project, which taking a range into projects refuses.
        Asked::new(Method::POST, format!("{meeting}/excerpts"), Needs::Edit).with_body(json!({
            "startWordId": sheriff.id,
            "endWordId": sheriff.id,
            "projectIds": [],
        })),
        // A range that ends before it starts.
        Asked::new(Method::POST, format!("{meeting}/black-outs"), Needs::Edit).with_body(json!({
            "startWordId": ships.id,
            "endWordId": sheriff.id,
        })),
        // A thread is no excerpt or black-out, which removing a mark refuses.
        Asked::new(
            Method::DELETE,
            format!("{meeting}/marks/{thread_id}"),
            Needs::Edit,
        ),
        Asked::new(
            Method::POST,
            format!("{meeting}/history/{}/undo", shared.entry.id),
            Needs::Manage,
        ),
        Asked::new(Method::POST, format!("{captured}/split"), Needs::Manage),
        Asked::new(
            Method::DELETE,
            format!("{meeting}/shares/{}", Uuid::now_v7()),
            Needs::Manage,
        ),
        Asked::new(
            Method::POST,
            format!("{meeting}/correction/approve"),
            Needs::Own,
        ),
        Asked::new(Method::POST, format!("{meeting}/hold"), Needs::Own),
        Asked::new(Method::POST, format!("{meeting}/retry"), Needs::Own),
        // Never segmented, and its run failed, so there is nothing to segment again.
        Asked::new(
            Method::POST,
            format!("{meeting}/done-reviewing"),
            Needs::Own,
        ),
        Asked::new(
            Method::POST,
            format!("/api/v1/meetings/duplicates/{}/merge", pair.id),
            Needs::Manage,
        ),
        Asked::new(
            Method::POST,
            format!("/api/v1/meetings/duplicates/{}/not-the-same", pair.id),
            Needs::Manage,
        ),
        Asked::new(Method::PUT, format!("{captured}/offset"), Needs::Own)
            .with_body(json!({ "offsetMs": 13 * 60 * 60 * 1000 })),
        Asked::new(Method::GET, format!("{meeting}/attachments"), Needs::Read),
        // JSON rather than the form a frame is sent as, which is read once who may send it is
        // decided.
        Asked::new(Method::POST, format!("{meeting}/frames"), Needs::Edit).with_body(json!({})),
        // A pin over a range that ends before it starts.
        Asked::new(Method::POST, format!("{meeting}/pins"), Needs::Edit).with_body(json!({
            "attachmentId": frame.id,
            "startWordId": ships.id,
            "endWordId": sheriff.id,
        })),
        Asked::new(
            Method::DELETE,
            format!("{meeting}/attachments/{}", frame.id),
            Needs::Edit,
        ),
        // Past the largest file a meeting takes.
        Asked::new(
            Method::POST,
            "/api/v1/meeting-uploads".to_owned(),
            Needs::Edit,
        )
        .with_body(json!({
            "meetingId": meeting_id,
            "attachment": { "name": "Launch plan.pdf", "sizeBytes": 6_000_000_000_u64 },
        })),
        Asked::new(
            Method::GET,
            format!("/api/v1/meeting-uploads/policy?meetingId={meeting_id}"),
            Needs::Own,
        ),
        Asked::new(
            Method::POST,
            "/api/v1/meeting-uploads".to_owned(),
            Needs::Own,
        )
        .with_body(json!({
            "meetingId": meeting_id,
            "transcript": { "name": "notes.vtt", "sizeBytes": 120 },
        })),
    ];

    for public in [false, true] {
        for relation in [Relation::Owner, Relation::SharedWith, Relation::Other] {
            for role in [Role::Member, Role::Admin, Role::Guest] {
                let cell = (role, relation, public);
                arrange(
                    &mut connection,
                    &[meeting_id, same_as.meeting_id],
                    grace,
                    cell,
                )
                .await;
                // Asked of the meeting's own visibility again, and of its owner, so each
                // reaches its own refusal once through.
                let owner = if relation == Relation::Owner {
                    TEST_PERSON_ID
                } else {
                    grace
                };
                let own_visibility = if public { "public" } else { "private" };
                let managing = [
                    Asked::new(Method::PUT, format!("{meeting}/visibility"), Needs::Manage)
                        .with_body(json!({ "visibility": own_visibility })),
                    Asked::new(
                        Method::PUT,
                        format!("{meeting}/shares/{owner}"),
                        Needs::Manage,
                    ),
                ];
                for asked in requests.iter().chain(&managing) {
                    if asked.path.ends_with("/retry") {
                        fail_what_is_queued(&mut connection).await;
                    }
                    let (status, body) = ask(
                        &state,
                        asked.method.clone(),
                        &asked.path,
                        asked.body.as_ref(),
                        asked.range,
                    )
                    .await;
                    let answered = String::from_utf8_lossy(&body);
                    let what = format!(
                        "{cell:?} {} {}: {status} {answered}",
                        asked.method, asked.path
                    );
                    match expected(asked.needs, role, relation, public) {
                        Expected::Allowed => assert!(
                            status != StatusCode::NOT_FOUND && status != StatusCode::FORBIDDEN,
                            "allowed through: {what}"
                        ),
                        Expected::NotFound => {
                            assert_eq!(status, StatusCode::NOT_FOUND, "not found: {what}");
                            // Not a byte of the recording, nor a word of what was said.
                            assert!(!answered.contains("2345"), "{what}");
                            assert!(!answered.contains("Sheriff"), "{what}");
                        }
                        Expected::Refused(code) => {
                            assert_eq!(status, StatusCode::FORBIDDEN, "refused: {what}");
                            let refused: Value = serde_json::from_slice(&body).expect("JSON");
                            assert_eq!(refused["code"], code, "{what}");
                        }
                    }
                    if asked.range.is_some()
                        && expected(asked.needs, role, relation, public) == Expected::Allowed
                    {
                        assert_eq!(status, StatusCode::PARTIAL_CONTENT, "{what}");
                        assert_eq!(body, b"2345", "{what}");
                    }
                }

                // Every item's page asks which thread raised it: one on a meeting they may not
                // read reads as none.
                let reads = expected(Needs::Read, role, relation, public) == Expected::Allowed;
                let (status, body) = ask(
                    &state,
                    Method::GET,
                    &format!("/api/v1/meetings/thread-items/{item_id}"),
                    None,
                    None,
                )
                .await;
                assert_eq!(status, StatusCode::OK, "{cell:?}");
                let opened: Value = serde_json::from_slice(&body).expect("JSON");
                assert_eq!(opened["thread"].is_object(), reads, "{cell:?}: {opened}");

                // And which task it was made of, which reads as none the same way.
                let (status, body) = ask(
                    &state,
                    Method::GET,
                    &format!("/api/v1/meetings/task-items/{task_item}"),
                    None,
                    None,
                )
                .await;
                assert_eq!(status, StatusCode::OK, "{cell:?}");
                let made_of: Value = serde_json::from_slice(&body).expect("JSON");
                assert_eq!(made_of["task"].is_object(), reads, "{cell:?}: {made_of}");

                // The item asking whether two meetings are one offers them only to whoever may
                // answer it, reading both and managing the one it asks about.
                let manages = expected(Needs::Manage, role, relation, public) == Expected::Allowed;
                let (status, body) = ask(
                    &state,
                    Method::GET,
                    &format!(
                        "/api/v1/meetings/duplicates?itemId={}",
                        duplicate_item.record.id
                    ),
                    None,
                    None,
                )
                .await;
                assert_eq!(status, StatusCode::OK, "{cell:?}");
                let offered: Value = serde_json::from_slice(&body).expect("JSON");
                assert_eq!(
                    offered["duplicate"].is_object(),
                    manages,
                    "{cell:?}: {offered}"
                );

                // The listing names exactly the meetings they may read.
                let (status, body) = ask(&state, Method::GET, "/api/v1/meetings", None, None).await;
                assert_eq!(status, StatusCode::OK, "{cell:?}");
                let listed: Value = serde_json::from_slice(&body).expect("JSON");
                let ids: Vec<&str> = listed["meetings"]
                    .as_array()
                    .expect("meetings")
                    .iter()
                    .map(|listed| listed["id"].as_str().expect("an id"))
                    .collect();
                assert_eq!(
                    ids.contains(&meeting_id.to_string().as_str()),
                    reads,
                    "{cell:?}: {listed}"
                );
            }
        }
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_meeting_answers_what_its_reader_may_do_with_it_and_who_reads_it() {
    let (url, mut connection) = migrated_database().await;
    let (storage, _files) = fake_storage().await;
    let state = app_state_with_storage(&url, storage).await;
    let grace = insert_grace(&mut connection).await;
    let (capture, _words) = built_meeting(&mut connection, &[("Ada", "Launch is Friday.")]).await;
    let meeting_id = capture.meeting_id;
    let path = format!("/api/v1/meetings/{meeting_id}");

    let read = |body: Vec<u8>| -> Value { serde_json::from_slice(&body).expect("JSON") };
    let (status, body) = ask(&state, Method::GET, &path, None, None).await;
    assert_eq!(status, StatusCode::OK);
    let answered = read(body);
    assert_eq!(
        answered["meeting"]["visibility"], "unchecked",
        "a new meeting is unchecked"
    );
    assert_eq!(answered["meeting"]["ownerId"], json!(TEST_PERSON_ID));
    assert_eq!(answered["meeting"]["canEdit"], true);
    assert_eq!(answered["meeting"]["canManage"], true);
    assert_eq!(answered["meeting"]["visibilityChange"], Value::Null);

    let (status, body) = ask(
        &state,
        Method::PUT,
        &format!("{path}/shares/{grace}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(read(body)["meeting"]["sharedWith"], json!([grace]));

    let (status, body) = ask(
        &state,
        Method::PUT,
        &format!("{path}/visibility"),
        Some(&json!({ "visibility": "public" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let opened = read(body);
    assert_eq!(opened["meeting"]["visibility"], "public");
    assert_eq!(opened["entry"]["kind"], "visibility_changed");
    assert_eq!(
        opened["meeting"]["visibilityChange"]["data"]["before"],
        "unchecked"
    );
    assert_eq!(
        opened["meeting"]["visibilityChange"]["data"]["after"],
        "public"
    );

    // Another member reads the public meeting and edits it, but neither manages nor runs it. The
    // test person is the workspace's first, and so its admin, until now.
    execute(
        &mut connection,
        format!("UPDATE users SET role = 'member' WHERE id = '{TEST_PERSON_ID}'"),
    )
    .await;
    execute(
        &mut connection,
        format!("UPDATE meetings SET created_by = '{grace}' WHERE id = '{meeting_id}'"),
    )
    .await;
    let (status, body) = ask(&state, Method::GET, &path, None, None).await;
    assert_eq!(status, StatusCode::OK);
    let as_member = read(body);
    assert_eq!(as_member["meeting"]["canEdit"], true);
    assert_eq!(as_member["meeting"]["canManage"], false);

    // Retrying its run is its owner's alone, as is all of its processing.
    let (status, body) = ask(&state, Method::POST, &format!("{path}/retry"), None, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(read(body)["code"], "owner_only");
    execute(
        &mut connection,
        format!("UPDATE meetings SET visibility = 'private' WHERE id = '{meeting_id}'"),
    )
    .await;
    let (status, _body) = ask(&state, Method::POST, &format!("{path}/retry"), None, None).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a private meeting nobody shared with them"
    );
}
