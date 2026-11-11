// Copyright © 2026 Jalapeno Labs

//! A meeting's review routes against Postgres (#234, #268): reading its final transcript with its
//! marks and captures, a capture as it came on the meeting's clock, an edit answering what it did,
//! its history naming each undo, undo refusing what moved on since, a capture merged in saying
//! where it came from, moved by hand and split back out, and every route answering the meeting's
//! owner alone.
//!
//! They are `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs
//! them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::Utc;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::meetings::duplicates;
use crate::models::meeting_duplicate::{self, NewMeetingDuplicate};
use crate::models::meeting_word::tests::{built_meeting, word};
use crate::models::user::SYSTEM_USER_ID;
use crate::realtime::EventBus;
use crate::routes;
use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database, signed_in_cookie};

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

/// Two of the test person's meetings of one call, the second merged into the first.
struct MergedPair {
    kept_id: Uuid,
    /// The capture the kept meeting was listed with.
    kept_capture: Uuid,
    merged_id: Uuid,
    /// The capture the merge brought in.
    merged_capture: Uuid,
}

/// Two meetings of one call, the second merged into the first as its person answered.
async fn merged_pair(connection: &mut AsyncPgConnection) -> MergedPair {
    let (kept, _) = built_meeting(connection, &[("Ada", "Launch is Friday.")]).await;
    let (merged, _) = built_meeting(connection, &[("Ada", "Launch is Friday.")]).await;
    execute(
        connection,
        format!(
            "UPDATE meetings SET title = 'Weekly sync, recorded twice' WHERE id = '{}'",
            merged.meeting_id
        ),
    )
    .await;
    let pair = meeting_duplicate::insert_new(
        connection,
        NewMeetingDuplicate {
            id: Uuid::now_v7(),
            meeting_id: merged.meeting_id,
            same_as_id: kept.meeting_id,
            action_item_id: None,
            created_by: SYSTEM_USER_ID,
        },
    )
    .await
    .expect("records")
    .expect("a new pair");
    duplicates::merge(connection, pair.id, TEST_PERSON_ID, Utc::now())
        .await
        .expect("merges");
    MergedPair {
        kept_id: kept.meeting_id,
        kept_capture: kept.id,
        merged_id: merged.meeting_id,
        merged_capture: merged.id,
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_merged_capture_says_where_it_came_from_and_splits_back_out() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let MergedPair {
        kept_id,
        kept_capture,
        merged_id,
        merged_capture,
    } = merged_pair(&mut connection).await;
    let words_path = format!("/api/v1/meetings/{kept_id}/words");
    let split_path = format!("/api/v1/meetings/{kept_id}/captures/{merged_capture}/split");

    // The meeting's own capture came from nowhere; the one merged in names its meeting.
    let (status, read) = send(&state, Method::GET, &words_path, None).await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["captures"][0]["id"], json!(kept_capture));
    assert_eq!(read["captures"][0]["mergedFrom"], Value::Null);
    assert_eq!(read["captures"][0]["offsetSource"], "listed");
    assert_eq!(read["captures"][1]["id"], json!(merged_capture));
    assert_eq!(
        read["captures"][1]["mergedFrom"],
        json!({ "meetingId": merged_id, "title": "Weekly sync, recorded twice" })
    );
    // The meeting merged away says where its captures went.
    let (status, away) = send(
        &state,
        Method::GET,
        &format!("/api/v1/meetings/{merged_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{away}");
    assert_eq!(away["meeting"]["mergedInto"], json!(kept_id));

    // Moved by hand, it answers the change Undo undoes, and reads as set by hand.
    let (status, moved) = send(
        &state,
        Method::PUT,
        &format!("/api/v1/meetings/{kept_id}/captures/{merged_capture}/offset"),
        Some(&json!({ "offsetMs": 1_500 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["entry"]["kind"], "offset_changed");
    assert_eq!(moved["capture"]["offsetMs"], 1_500);
    assert_eq!(moved["capture"]["offsetSource"], "hand");
    let (_, read) = send(&state, Method::GET, &words_path, None).await;
    assert_eq!(read["captures"][1]["offsetMs"], 1_500);
    assert_eq!(read["captures"][1]["offsetSource"], "hand");

    // Splitting it now would overwrite that move, so it waits until the move is undone.
    let (status, refused) = send(&state, Method::POST, &split_path, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "meeting_changed");
    let moved_id = moved["entry"]["id"].as_str().expect("an entry id");
    let (status, _) = send(
        &state,
        Method::POST,
        &format!("/api/v1/meetings/{kept_id}/history/{moved_id}/undo"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Split, it answers the change and the meeting kept; both meetings are their own again.
    let (status, split) = send(&state, Method::POST, &split_path, None).await;
    assert_eq!(status, StatusCode::OK, "{split}");
    assert_eq!(split["entry"]["kind"], "split");
    assert_eq!(split["meeting"]["id"], json!(kept_id));
    let (_, read) = send(&state, Method::GET, &words_path, None).await;
    let captures = read["captures"].as_array().expect("captures");
    assert_eq!(captures.len(), 1, "{read}");
    assert_eq!(captures[0]["id"], json!(kept_capture));
    let (_, away) = send(
        &state,
        Method::GET,
        &format!("/api/v1/meetings/{merged_id}"),
        None,
    )
    .await;
    assert_eq!(away["meeting"]["mergedInto"], Value::Null);

    // A capture the meeting was listed with was never merged in.
    let (status, own) = send(
        &state,
        Method::POST,
        &format!("/api/v1/meetings/{kept_id}/captures/{kept_capture}/split"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{own}");
    assert_eq!(own["code"], "capture_not_merged");

    // Undoing the split merges them again, and the capture names its meeting once more.
    let split_id = split["entry"]["id"].as_str().expect("an entry id");
    let (status, merged_again) = send(
        &state,
        Method::POST,
        &format!("/api/v1/meetings/{kept_id}/history/{split_id}/undo"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{merged_again}");
    assert_eq!(merged_again["entry"]["kind"], "merged");
    let (_, read) = send(&state, Method::GET, &words_path, None).await;
    assert_eq!(
        read["captures"][1]["mergedFrom"]["meetingId"],
        json!(merged_id)
    );
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
    let offset = json!({ "offsetMs": 1_000 });
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
        (
            Method::PUT,
            format!(
                "/api/v1/meetings/{meeting_id}/captures/{}/offset",
                capture.id
            ),
            Some(&offset),
        ),
        (
            Method::POST,
            format!(
                "/api/v1/meetings/{meeting_id}/captures/{}/split",
                capture.id
            ),
            None,
        ),
    ];
    for (method, path, body) in requests {
        let (status, answer) = send(&state, method, &path, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {answer}");
    }
}
