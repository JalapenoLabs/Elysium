// Copyright © 2026 Jalapeno Labs

//! A meeting's review routes against Postgres (#234): reading its final transcript with its marks
//! and captures, a capture as it came on the meeting's clock, an edit answering what it did, its
//! history naming each undo, undo refusing what moved on since, and every route answering the
//! meeting's owner alone.
//!
//! They are `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs
//! them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::SecretString;
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::models::meeting_word::tests::{built_meeting, word};
use crate::models::project::ProjectScope;
use crate::models::storage_location::{
    self, BunnyStorageRegion, NewStorageLocation, StorageProvider,
};
use crate::models::user::SYSTEM_USER_ID;
use crate::realtime::EventBus;
use crate::routes;
use crate::storage::tests::{Files, PASSWORD, StoredFile, fake_storage};
use crate::test_support::{
    TEST_PERSON_ID, app_state, app_state_with_storage, migrated_database, signed_in_cookie,
};

/// Sends `method` to `path` as the signed-in test person, and answers the status and body.
async fn send(
    state: &crate::state::AppState,
    method: Method,
    path: &str,
    body: Option<&Value>,
) -> (StatusCode, Value) {
    let cookie = signed_in_cookie(state).await;
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::COOKIE, cookie)
        .header(header::ORIGIN, "http://localhost:4000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
        .expect("a request builds");
    let response = routes::router(state)
        .with_state(state.clone())
        .oneshot(request)
        .await
        .expect("the router answers");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("the body reads");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// The texts of a words list as the routes answer it, in order.
fn texts(words: &Value) -> Vec<&str> {
    words
        .as_array()
        .expect("a list of words")
        .iter()
        .map(|word| word["text"].as_str().expect("a word's text"))
        .collect()
}

async fn execute(connection: &mut AsyncPgConnection, statement: String) {
    diesel::sql_query(statement)
        .execute(connection)
        .await
        .expect("the statement runs");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn an_edit_answers_what_it_did_and_its_undo_puts_the_words_back() {
    let (url, mut connection) = migrated_database().await;
    let mut state = app_state(&url).await;
    state.events = EventBus::in_process();
    let mut heard = state.events.subscribe();
    let (capture, words) = built_meeting(
        &mut connection,
        &[("Ada", "Launch is Fryday."), ("Grace", "Agreed.")],
    )
    .await;
    let meeting_id = capture.meeting_id;
    let fryday = word(&words, "Fryday.");

    let (status, read) = send(
        &state,
        Method::GET,
        &format!("/api/v1/meetings/{meeting_id}/words"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(
        texts(&read["words"]),
        vec!["Launch", "is", "Fryday.", "Agreed."]
    );
    assert_eq!(read["marks"], json!([]));
    assert_eq!(read["captures"][0]["id"], json!(capture.id));

    let (status, edited) = send(
        &state,
        Method::POST,
        &format!("/api/v1/meetings/{meeting_id}/edits"),
        Some(&json!({
            "startWordId": fryday.id,
            "endWordId": fryday.id,
            "text": "on Friday.",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(edited["entry"]["kind"], "edited");
    assert_eq!(edited["change"]["removedWordIds"], json!([fryday.id]));
    assert_eq!(texts(&edited["change"]["words"]), vec!["on", "Friday."]);
    // The new words slot in just after the word they replace, before what followed it.
    let after = edited["change"]["words"][0]["position"]
        .as_i64()
        .expect("a position");
    assert!(after > fryday.position && after < word(&words, "Agreed.").position);
    let marks = edited["change"]["marks"].as_array().expect("marks");
    assert_eq!(marks.len(), 1, "the edit's own mark: {marks:?}");
    assert_eq!(marks[0]["kind"], "edit");
    assert_eq!(marks[0]["agent"], Value::Null);

    // Announced with the entry it recorded, so the page that made it skips reading it again, and
    // never with what the meeting says.
    let payload = heard.try_recv().expect("the edit is announced");
    assert!(
        !payload.contains("Friday"),
        "never what a meeting says: {payload}"
    );
    let event: Value = serde_json::from_str(&payload).expect("JSON");
    assert_eq!(event["type"], "meeting.changed");
    assert_eq!(event["data"]["id"], json!(meeting_id));
    assert_eq!(event["data"]["entryId"], edited["entry"]["id"]);

    let (_, read) = send(
        &state,
        Method::GET,
        &format!("/api/v1/meetings/{meeting_id}/words"),
        None,
    )
    .await;
    assert_eq!(
        texts(&read["words"]),
        vec!["Launch", "is", "on", "Friday.", "Agreed."]
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn history_names_each_undo_and_undo_refuses_what_moved_on() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let (capture, words) = built_meeting(&mut connection, &[("Ada", "Launch is Fryday.")]).await;
    let meeting_id = capture.meeting_id;
    let fryday = word(&words, "Fryday.");
    let (status, edited) = send(
        &state,
        Method::POST,
        &format!("/api/v1/meetings/{meeting_id}/edits"),
        Some(&json!({ "startWordId": fryday.id, "endWordId": fryday.id, "text": "Friday." })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{edited}");

    let entry_id = edited["entry"]["id"].as_str().expect("an entry id");
    let (status, undone) = send(
        &state,
        Method::POST,
        &format!("/api/v1/meetings/{meeting_id}/history/{entry_id}/undo"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{undone}");
    assert_eq!(undone["entry"]["undid"], json!(entry_id));
    assert_eq!(texts(&undone["change"]["words"]), vec!["Fryday."]);
    assert_eq!(undone["change"]["marks"], json!([]));

    // History reads newest first, each undo naming what it undid.
    let (status, history) = send(
        &state,
        Method::GET,
        &format!("/api/v1/meetings/{meeting_id}/history"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{history}");
    let kinds: Vec<&str> = history["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|entry| entry["kind"].as_str().expect("a kind"))
        .collect();
    assert_eq!(kinds, vec!["edited", "edited", "built"]);
    assert_eq!(history["entries"][0]["undid"], json!(entry_id));
    assert_eq!(history["entries"][1]["undid"], Value::Null);

    // What it changed was put back already, so undoing it again would overwrite that.
    let (status, again) = send(
        &state,
        Method::POST,
        &format!("/api/v1/meetings/{meeting_id}/history/{entry_id}/undo"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{again}");
    assert_eq!(again["code"], "meeting_changed_since");

    let built_id = history["entries"][2]["id"].as_str().expect("an id");
    let (status, refused) = send(
        &state,
        Method::POST,
        &format!("/api/v1/meetings/{meeting_id}/history/{built_id}/undo"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "meeting_change_not_undoable");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn an_edit_is_refused_with_no_words_or_over_words_replaced_meanwhile() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let (capture, words) = built_meeting(&mut connection, &[("Ada", "Launch is Fryday.")]).await;
    let meeting_id = capture.meeting_id;
    let fryday = word(&words, "Fryday.");
    let path = format!("/api/v1/meetings/{meeting_id}/edits");
    let over_fryday =
        |text: &str| json!({ "startWordId": fryday.id, "endWordId": fryday.id, "text": text });

    let (status, empty) = send(&state, Method::POST, &path, Some(&over_fryday("  \n "))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{empty}");
    assert_eq!(empty["code"], "meeting_edit_empty");

    let (status, _) = send(&state, Method::POST, &path, Some(&over_fryday("Friday."))).await;
    assert_eq!(status, StatusCode::OK);
    // A page still showing the old word edits a range that no longer reads.
    let (status, outdated) = send(&state, Method::POST, &path, Some(&over_fryday("Monday."))).await;
    assert_eq!(status, StatusCode::CONFLICT, "{outdated}");
    assert_eq!(outdated["code"], "meeting_words_changed");

    let backwards = json!({
        "startWordId": word(&words, "is").id,
        "endWordId": word(&words, "Launch").id,
        "text": "Friday.",
    });
    let (status, _) = send(&state, Method::POST, &path, Some(&backwards)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_capture_reads_as_it_came_on_the_meetings_clock() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let (capture, _) = built_meeting(
        &mut connection,
        &[("Ada", "Launch is Fryday."), ("Grace", "Agreed.")],
    )
    .await;
    let meeting_id = capture.meeting_id;
    execute(
        &mut connection,
        format!(
            "UPDATE meeting_captures SET offset_ms = 500 WHERE id = '{}'",
            capture.id
        ),
    )
    .await;

    let (status, read) = send(
        &state,
        Method::GET,
        &format!(
            "/api/v1/meetings/{meeting_id}/captures/{}/transcript",
            capture.id
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["transcript"]["basedOn"], "source");
    assert_eq!(
        read["transcript"]["lines"],
        json!([
            { "startMs": 500, "endMs": 2_500, "speaker": "Ada", "text": "Launch is Fryday." },
            { "startMs": 2_500, "endMs": 4_500, "speaker": "Grace", "text": "Agreed." },
        ])
    );

    // A capture is read only through its own meeting.
    let (other, _) = built_meeting(&mut connection, &[("Ada", "Elsewhere.")]).await;
    let (status, _) = send(
        &state,
        Method::GET,
        &format!(
            "/api/v1/meetings/{meeting_id}/captures/{}/transcript",
            other.id
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn every_review_route_answers_the_meetings_owner_alone() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let (capture, words) = built_meeting(&mut connection, &[("Ada", "Launch is Fryday.")]).await;
    let meeting_id = capture.meeting_id;
    let fryday = word(&words, "Fryday.");
    let (status, edited) = send(
        &state,
        Method::POST,
        &format!("/api/v1/meetings/{meeting_id}/edits"),
        Some(&json!({ "startWordId": fryday.id, "endWordId": fryday.id, "text": "Friday." })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    let entry_id = edited["entry"]["id"].as_str().expect("an entry id");

    // Someone else's meeting, as far as the test person is concerned.
    execute(
        &mut connection,
        format!("UPDATE meetings SET created_by = '{SYSTEM_USER_ID}' WHERE id = '{meeting_id}'"),
    )
    .await;
    let replacement = Uuid::parse_str(
        edited["change"]["words"][0]["id"]
            .as_str()
            .expect("a word id"),
    )
    .expect("a uuid");
    let edit = json!({ "startWordId": replacement, "endWordId": replacement, "text": "Monday." });
    let requests = [
        (
            Method::GET,
            format!("/api/v1/meetings/{meeting_id}/words"),
            None,
        ),
        (
            Method::POST,
            format!("/api/v1/meetings/{meeting_id}/edits"),
            Some(&edit),
        ),
        (
            Method::GET,
            format!(
                "/api/v1/meetings/{meeting_id}/captures/{}/transcript",
                capture.id
            ),
            None,
        ),
        (
            Method::GET,
            format!("/api/v1/meetings/{meeting_id}/history"),
            None,
        ),
        (
            Method::POST,
            format!("/api/v1/meetings/{meeting_id}/history/{entry_id}/undo"),
            None,
        ),
    ];
    for (method, path, body) in requests {
        let (status, answer) = send(&state, method, &path, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {answer}");
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_thread_is_started_replied_to_resolved_and_removed_by_the_meetings_owner_alone() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let grace = insert_grace(&mut connection).await;
    let (capture, words) = built_meeting(&mut connection, &[("Ada", "Sheriff ships Friday")]).await;
    let meeting_id = capture.meeting_id;
    let sheriff = word(&words, "Sheriff");
    let threads = format!("/api/v1/meetings/{meeting_id}/threads");

    let (status, started) = send(
        &state,
        Method::POST,
        &threads,
        Some(&json!({
            "startWordId": sheriff.id,
            "endWordId": sheriff.id,
            "body": "Is this Sharath, Grace?",
            "mentions": [grace],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    assert_eq!(started["entry"]["kind"], "marked");
    let thread_id = started["comment"]["threadId"]
        .as_str()
        .expect("a thread id")
        .to_owned();
    assert_eq!(started["comment"]["mentions"], json!([grace]));
    let thread = started["change"]["marks"]
        .as_array()
        .expect("marks")
        .iter()
        .find(|mark| mark["id"] == json!(thread_id))
        .expect("the thread's mark");
    assert_eq!(thread["kind"], "comment");
    assert_eq!(thread["data"]["resolved"], false);

    let (status, replied) = send(
        &state,
        Method::POST,
        &format!("{threads}/{thread_id}/comments"),
        Some(&json!({ "body": "Answering myself: it is." })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{replied}");
    assert!(replied.get("entry").is_none());
    let (status, listed) = send(&state, Method::GET, &threads, None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["comments"][0]["body"], "Is this Sharath, Grace?");
    assert_eq!(listed["comments"][1]["body"], "Answering myself: it is.");

    // The item the mention raised opens the thread from its page.
    let item_id = thread_item_for(&mut connection, grace).await;
    let item_path = format!("/api/v1/meetings/thread-items/{item_id}");
    let (status, opened) = send(&state, Method::GET, &item_path, None).await;
    assert_eq!(status, StatusCode::OK, "{opened}");
    assert_eq!(
        opened,
        json!({ "thread": { "meetingId": meeting_id, "threadId": thread_id } })
    );

    let resolved_path = format!("{threads}/{thread_id}/resolved");
    let (status, resolved) = send(&state, Method::POST, &resolved_path, None).await;
    assert_eq!(status, StatusCode::OK, "{resolved}");
    assert_eq!(resolved["entry"]["kind"], "thread_resolved");
    let (status, again) = send(&state, Method::POST, &resolved_path, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{again}");
    assert_eq!(again["code"], "meeting_thread_unchanged");
    let (status, reopened) = send(&state, Method::DELETE, &resolved_path, None).await;
    assert_eq!(status, StatusCode::OK, "{reopened}");
    assert_eq!(reopened["entry"]["kind"], "thread_reopened");

    for (body, code) in [
        (json!({ "body": "  " }), "meeting_comment_empty"),
        (
            json!({ "body": "Hello?", "mentions": [Uuid::now_v7()] }),
            "meeting_comment_mention",
        ),
    ] {
        let (status, refused) = send(
            &state,
            Method::POST,
            &format!("{threads}/{thread_id}/comments"),
            Some(&body),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
        assert_eq!(refused["code"], code);
    }

    let (status, removed) = send(
        &state,
        Method::DELETE,
        &format!("{threads}/{thread_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{removed}");
    assert_eq!(removed["entry"]["kind"], "unmarked");
    let (status, listed) = send(&state, Method::GET, &threads, None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["comments"], json!([]));
}

/// Grace Hopper, an active member someone can mention.
async fn insert_grace(connection: &mut AsyncPgConnection) -> Uuid {
    let grace = Uuid::now_v7();
    execute(
        connection,
        format!(
            "INSERT INTO users (id, kratos_identity_id, email, name, role, status, approved_at) \
             VALUES ('{grace}', gen_random_uuid(), 'grace@example.com', 'Grace Hopper', 'member', \
             'active', now())"
        ),
    )
    .await;
    grace
}

/// The inbox item a thread raised for `person`.
async fn thread_item_for(connection: &mut AsyncPgConnection, person: Uuid) -> Uuid {
    use diesel::{ExpressionMethods, QueryDsl};

    use crate::database::schema::meeting_thread_items;
    meeting_thread_items::table
        .filter(meeting_thread_items::person_id.eq(person))
        .select(meeting_thread_items::action_item_id)
        .first(connection)
        .await
        .expect("a thread raised an item for them")
}

/// The recording a capture keeps in the fake zone: its storage location, and its bytes stored at
/// the path the capture names.
async fn keep_recording(
    state: &crate::state::AppState,
    connection: &mut AsyncPgConnection,
    files: &Files,
    capture_id: Uuid,
    bytes: &[u8],
) {
    let location = storage_location::create(
        connection,
        &state.cipher,
        &NewStorageLocation {
            created_by: TEST_PERSON_ID,
            name: "Recordings".to_owned(),
            provider: StorageProvider::Bunny {
                zone: "files".to_owned(),
                region: BunnyStorageRegion::Frankfurt,
            },
            path_prefix: String::new(),
            storage_limit_bytes: None,
            access_key: SecretString::from(PASSWORD),
            projects: ProjectScope::All,
        },
    )
    .await
    .expect("created");
    let path = format!("meetings/recordings/{capture_id}.mp4");
    execute(
        connection,
        format!(
            "UPDATE meeting_captures SET storage_location_id = '{}', recording_path = '{path}', \
             recording_content_type = 'video/mp4' WHERE id = '{capture_id}'",
            location.id
        ),
    )
    .await;
    files.lock().expect("lock").insert(
        path,
        StoredFile {
            headers: axum::http::HeaderMap::new(),
            bytes: bytes.to_vec(),
        },
    );
}

/// Asks for `path` as the signed-in test person, with `range` when given, and answers the
/// status, headers, and bytes.
async fn play(
    state: &crate::state::AppState,
    path: &str,
    range: Option<&str>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let cookie = signed_in_cookie(state).await;
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(path)
        .header(header::COOKIE, cookie);
    if let Some(range) = range {
        request = request.header(header::RANGE, range);
    }
    let response = routes::router(state)
        .with_state(state.clone())
        .oneshot(request.body(Body::empty()).expect("a request builds"))
        .await
        .expect("the router answers");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("the body reads");
    (status, headers, bytes.to_vec())
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn every_thread_route_answers_the_meetings_owner_alone() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let grace = insert_grace(&mut connection).await;
    let (capture, words) = built_meeting(&mut connection, &[("Ada", "Sheriff ships Friday")]).await;
    let meeting_id = capture.meeting_id;
    let sheriff = word(&words, "Sheriff");
    let threads = format!("/api/v1/meetings/{meeting_id}/threads");
    let start = json!({
        "startWordId": sheriff.id,
        "endWordId": sheriff.id,
        "body": "Grace?",
        "mentions": [grace],
    });
    let (status, started) = send(&state, Method::POST, &threads, Some(&start)).await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let thread_id = started["comment"]["threadId"]
        .as_str()
        .expect("a thread id")
        .to_owned();
    let item_id = thread_item_for(&mut connection, grace).await;
    let resolved_path = format!("{threads}/{thread_id}/resolved");

    // Someone else's meeting, as far as the test person is concerned.
    execute(
        &mut connection,
        format!("UPDATE meetings SET created_by = '{SYSTEM_USER_ID}' WHERE id = '{meeting_id}'"),
    )
    .await;
    let reply = json!({ "body": "Mine?" });
    let requests = [
        (Method::GET, threads.clone(), None),
        (Method::POST, threads.clone(), Some(&start)),
        (
            Method::POST,
            format!("{threads}/{thread_id}/comments"),
            Some(&reply),
        ),
        (Method::POST, resolved_path.clone(), None),
        (Method::DELETE, resolved_path, None),
        (Method::DELETE, format!("{threads}/{thread_id}"), None),
    ];
    for (method, path, body) in requests {
        let (status, answer) = send(&state, method, &path, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {answer}");
    }
    // Every item's page asks which thread raised it, so a thread not theirs reads as none.
    let item_path = format!("/api/v1/meetings/thread-items/{item_id}");
    let (status, opened) = send(&state, Method::GET, &item_path, None).await;
    assert_eq!(status, StatusCode::OK, "{opened}");
    assert_eq!(opened, json!({ "thread": null }));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_captures_recording_plays_a_range_at_a_time_for_its_owner_alone() {
    let (url, mut connection) = migrated_database().await;
    let (storage, files) = fake_storage().await;
    let state = app_state_with_storage(&url, storage).await;
    let (capture, _words) = built_meeting(&mut connection, &[("Ada", "Launch is Friday.")]).await;
    let meeting_id = capture.meeting_id;
    let media = format!(
        "/api/v1/meetings/{meeting_id}/captures/{}/media",
        capture.id
    );

    // No recording kept yet: nothing to play.
    let (status, _headers, _bytes) = play(&state, &media, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    keep_recording(&state, &mut connection, &files, capture.id, b"0123456789").await;
    let (status, read) = send(
        &state,
        Method::GET,
        &format!("/api/v1/meetings/{meeting_id}/words"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["captures"][0]["hasRecording"], json!(true));
    assert_eq!(read["captures"][0]["recordingContentType"], "video/mp4");

    let (status, headers, bytes) = play(&state, &media, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, b"0123456789");
    assert_eq!(headers[header::ACCEPT_RANGES], "bytes");
    assert_eq!(headers[header::CONTENT_TYPE], "video/mp4");
    assert_eq!(headers[header::CONTENT_LENGTH], "10");

    let (status, headers, bytes) = play(&state, &media, Some("bytes=6-")).await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(bytes, b"6789");
    assert_eq!(headers[header::CONTENT_RANGE], "bytes 6-9/10");
    assert_eq!(headers[header::CONTENT_LENGTH], "4");

    let (status, headers, bytes) = play(&state, &media, Some("bytes=10-")).await;
    assert_eq!(status, StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(headers[header::CONTENT_RANGE], "bytes */10");
    assert!(bytes.is_empty());

    // Another meeting's capture is not this meeting's to play.
    let (other, _other_words) = built_meeting(&mut connection, &[("Grace", "Elsewhere.")]).await;
    let (status, _headers, _bytes) = play(
        &state,
        &format!(
            "/api/v1/meetings/{}/captures/{}/media",
            other.meeting_id, capture.id
        ),
        Some("bytes=0-"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Someone else's meeting: neither the whole recording nor any part of it.
    execute(
        &mut connection,
        format!("UPDATE meetings SET created_by = '{SYSTEM_USER_ID}' WHERE id = '{meeting_id}'"),
    )
    .await;
    for range in [None, Some("bytes=0-"), Some("bytes=2-5"), Some("bytes=-3")] {
        let (status, headers, bytes) = play(&state, &media, range).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{range:?}");
        assert!(headers.get(header::CONTENT_RANGE).is_none(), "{range:?}");
        assert!(
            !bytes.windows(3).any(|window| window == b"234"),
            "{range:?}"
        );
    }
}
