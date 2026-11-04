// Copyright © 2026 Jalapeno Labs

//! Asking the knowledge base, driven through the router (#97): a question the search answers is
//! queued with what the search offered, one it does not is answered at once without a turn, an
//! ask is its asker's alone, each person is held to their asks, guests are refused, and the
//! engine's side takes, loads, and ends asks as the answerer does.
//!
//! The budget is counted from the asks recorded, never from a clock that moves while the test
//! runs: a person who asked their fill within the hour is refused however fast the test is.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use super::wait_for_answer;
use crate::auth::MemberUser;
use crate::knowledge::ask::{ASKS_PER_WINDOW, WAITING_PER_PERSON};
use crate::models::knowledge_ask::{
    self, Answer, KnowledgeAskState, NewKnowledgeAsk, PassageReference,
};
use crate::models::user;
use crate::routes;
use crate::routes::v1::knowledge::tests::{
    at, coworker, document_of_record, get, record, set_role,
};
use crate::state::AppState;
use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database, signed_in_cookie};

/// The router's answer to `POST /api/v1/knowledge/asks` with `body`, as the test person.
async fn post_ask(state: &AppState, body: &Value) -> (StatusCode, Value) {
    let cookie = signed_in_cookie(state).await;
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/knowledge/asks")
        .header(header::COOKIE, cookie)
        .header(header::ORIGIN, "http://localhost:4000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("a request builds");
    let response = routes::router(state)
        .with_state(state.clone())
        .oneshot(request)
        .await
        .expect("the router answers");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("the body reads");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// An ask recorded for `asker` as the API records one: queued when it offers a passage,
/// answered with nothing to say when it offers none.
async fn recorded_ask(
    connection: &mut AsyncPgConnection,
    asker: Uuid,
    offered: Vec<PassageReference>,
) -> Uuid {
    knowledge_ask::insert(
        connection,
        NewKnowledgeAsk {
            id: Uuid::now_v7(),
            created_by: asker,
            question: "When do we deploy?".to_owned(),
            filters: json!({}),
            offered,
            searched_by_meaning: false,
        },
    )
    .await
    .expect("the ask is recorded")
    .id
}

/// The first passage of the document a record became.
async fn first_passage_of(connection: &mut AsyncPgConnection, record_id: Uuid) -> PassageReference {
    PassageReference {
        document_id: document_of_record(connection, record_id).await,
        position: 0,
    }
}

async fn test_member(connection: &mut AsyncPgConnection) -> MemberUser {
    let person = user::find(connection, TEST_PERSON_ID)
        .await
        .expect("the test person exists");
    MemberUser::check(person).expect("the test person is a member")
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_question_the_search_answers_is_queued_with_what_it_offered() {
    let (url, mut connection) = migrated_database().await;
    // The test state's embedding service answers nothing, so the search ranks by words alone.
    let state = app_state(&url).await;
    let record_id = record(
        &mut connection,
        TEST_PERSON_ID,
        "slack",
        at(1),
        "Sign-in loop",
        "WAY-123 is the sign-in loop on Safari; the fix is in review.",
    )
    .await;

    let (status, body) = post_ask(
        &state,
        &json!({ "question": "  What is WAY-123?  ", "sources": ["slack"] }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    let asked = &body["ask"];
    assert_eq!(asked["state"], "queued");
    assert_eq!(asked["question"], "What is WAY-123?");
    assert_eq!(asked["filters"], json!({ "sources": ["slack"] }));
    assert_eq!(asked["searchedByMeaning"], false);
    assert_eq!(asked["answer"], Value::Null);
    let id: Uuid = serde_json::from_value(asked["id"].clone()).expect("an id");
    let stored = knowledge_ask::get_for(&mut connection, id, TEST_PERSON_ID)
        .await
        .expect("the ask reads")
        .expect("the ask is the test person's");
    assert_eq!(
        stored.offered(),
        [first_passage_of(&mut connection, record_id).await],
        "what the search offered, by reference"
    );

    let (status, read) = get(&state, &format!("/api/v1/knowledge/asks/{id}")).await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["ask"]["id"], asked["id"]);
    let (status, listed) = get(&state, "/api/v1/knowledge/asks").await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["asks"][0]["id"], asked["id"]);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_question_nothing_answers_is_answered_at_once_without_a_turn() {
    let (url, _connection) = migrated_database().await;
    let state = app_state(&url).await;

    let (status, body) = post_ask(&state, &json!({ "question": "What is ZZZ-999?" })).await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["ask"]["state"], "answered");
    assert_eq!(
        body["ask"]["answer"],
        json!({ "claims": [], "sources": [], "unsupportedClaims": 0 })
    );
    assert_eq!(body["ask"]["startedAt"], Value::Null, "no turn ran");
    assert!(body["ask"]["finishedAt"].is_string());
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn an_ask_is_its_askers_alone() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let other = coworker(&mut connection, "Robin Park").await;
    let theirs = recorded_ask(&mut connection, other, Vec::new()).await;
    let mine = recorded_ask(&mut connection, TEST_PERSON_ID, Vec::new()).await;

    let (status, _) = get(&state, &format!("/api/v1/knowledge/asks/{theirs}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, listed) = get(&state, "/api/v1/knowledge/asks").await;
    let ids: Vec<&Value> = listed["asks"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|asked| &asked["id"])
        .collect();
    assert_eq!(ids, [&json!(mine)]);

    // Waiting on someone else's ask is refused the same way, at once.
    let member = test_member(&mut connection).await;
    let waited = wait_for_answer(&state.database, &member, theirs, Duration::from_secs(5)).await;
    assert!(matches!(waited, Err(crate::errors::ApiError::NotFound)));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_wait_ends_with_the_answer_or_at_its_bound() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let record_id = record(
        &mut connection,
        TEST_PERSON_ID,
        "slack",
        at(1),
        "Plan",
        "A plan.",
    )
    .await;
    let offered = vec![first_passage_of(&mut connection, record_id).await];
    let answered = recorded_ask(&mut connection, TEST_PERSON_ID, Vec::new()).await;
    let queued = recorded_ask(&mut connection, TEST_PERSON_ID, offered).await;
    let member = test_member(&mut connection).await;

    let done = wait_for_answer(&state.database, &member, answered, Duration::from_mins(5))
        .await
        .expect("the ask reads");
    assert_eq!(done.state, KnowledgeAskState::Answered, "answered at once");

    let waiting = wait_for_answer(&state.database, &member, queued, Duration::from_millis(50))
        .await
        .expect("the ask reads");
    assert_eq!(
        waiting.state,
        KnowledgeAskState::Queued,
        "as it stands once the wait is up"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_person_who_asked_their_fill_this_hour_is_refused() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    for _ in 0..ASKS_PER_WINDOW {
        recorded_ask(&mut connection, TEST_PERSON_ID, Vec::new()).await;
    }
    // Someone else's asks are theirs to spend.
    let other = coworker(&mut connection, "Robin Park").await;
    recorded_ask(&mut connection, other, Vec::new()).await;

    let (status, refused) = post_ask(&state, &json!({ "question": "Anything?" })).await;

    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{refused}");
    assert_eq!(refused["code"], "knowledge_ask_rate_limited");
    let message = refused["message"].as_str().expect("a message");
    assert!(message.contains("try again in"), "{message}");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_person_with_questions_still_waiting_waits_for_one() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let record_id = record(
        &mut connection,
        TEST_PERSON_ID,
        "slack",
        at(1),
        "Plan",
        "A plan.",
    )
    .await;
    let offered = first_passage_of(&mut connection, record_id).await;
    for _ in 0..WAITING_PER_PERSON {
        recorded_ask(&mut connection, TEST_PERSON_ID, vec![offered]).await;
    }

    let (status, refused) = post_ask(&state, &json!({ "question": "Anything?" })).await;

    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{refused}");
    assert_eq!(refused["code"], "knowledge_ask_rate_limited");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_question_is_checked_and_a_guest_refused() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;

    for body in [
        json!({ "question": "   " }),
        json!({ "question": "a".repeat(501) }),
        json!({ "question": "x", "sources": ["Slack"] }),
        json!({ "question": "x", "from": "2026-11-04T10:00:00Z", "to": "2026-11-04T09:00:00Z" }),
    ] {
        let (status, refused) = post_ask(&state, &body).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}: {refused}"
        );
    }
    let (status, _) = post_ask(&state, &json!({ "question": "x", "limit": 3 })).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an unknown field is refused"
    );

    let mine = recorded_ask(&mut connection, TEST_PERSON_ID, Vec::new()).await;
    set_role(&mut connection, "guest").await;
    let (status, refused) = post_ask(&state, &json!({ "question": "Anything?" })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(refused["code"], "members_only");
    for path in [
        "/api/v1/knowledge/asks".to_owned(),
        format!("/api/v1/knowledge/asks/{mine}"),
    ] {
        let (status, refused) = get(&state, &path).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(refused["code"], "members_only", "{path}");
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn the_engine_takes_asks_oldest_first_and_ends_them() {
    let (_url, mut connection) = migrated_database().await;
    let kept = record(
        &mut connection,
        TEST_PERSON_ID,
        "slack",
        at(1),
        "Plan",
        "A plan.",
    )
    .await;
    let removed = record(
        &mut connection,
        TEST_PERSON_ID,
        "slack",
        at(2),
        "Gone",
        "Soon gone.",
    )
    .await;
    let kept = first_passage_of(&mut connection, kept).await;
    let gone = first_passage_of(&mut connection, removed).await;
    let first = recorded_ask(&mut connection, TEST_PERSON_ID, vec![gone, kept]).await;
    let second = recorded_ask(&mut connection, TEST_PERSON_ID, vec![kept]).await;
    let third = recorded_ask(&mut connection, TEST_PERSON_ID, vec![kept]).await;
    diesel::sql_query(format!(
        "DELETE FROM timeline_records WHERE id = '{removed}'"
    ))
    .execute(&mut connection)
    .await
    .expect("the record is removed");

    let taken = knowledge_ask::take_queued(&mut connection, 2)
        .await
        .expect("asks are taken");
    let taken_ids: Vec<Uuid> = taken.iter().map(|asked| asked.id).collect();
    assert_eq!(taken_ids, [first, second], "oldest first, as many as fit");
    assert!(
        taken
            .iter()
            .all(|asked| asked.state == KnowledgeAskState::Running)
    );

    // A passage whose record was removed is offered to nobody; the rest keep their labels.
    let offered = knowledge_ask::offered_passages(&mut connection, &taken[0].offered())
        .await
        .expect("the passages load");
    assert_eq!(offered.len(), 1);
    assert_eq!(offered[0].label, 2);
    assert_eq!(offered[0].citation.document_id, kept.document_id);
    assert_eq!(offered[0].citation.title, "Plan");
    assert!(offered[0].content.contains("A plan."));

    let answered = knowledge_ask::finish(&mut connection, first, &Answer::default())
        .await
        .expect("the answer records")
        .expect("the ask was running");
    assert_eq!(answered.state, KnowledgeAskState::Answered);
    assert!(
        knowledge_ask::finish(&mut connection, third, &Answer::default())
            .await
            .expect("the answer records")
            .is_none(),
        "only a running ask is answered"
    );

    // What a stopped engine left running fails as the next one starts; the queued one waits.
    let failed = knowledge_ask::fail_running(&mut connection, "Elysium stopped")
        .await
        .expect("running asks fail");
    let failed_ids: Vec<Uuid> = failed.iter().map(|asked| asked.id).collect();
    assert_eq!(failed_ids, [second]);
    assert_eq!(failed[0].error.as_deref(), Some("Elysium stopped"));
    let waiting = knowledge_ask::get_for(&mut connection, third, TEST_PERSON_ID)
        .await
        .expect("the ask reads")
        .expect("the ask exists");
    assert_eq!(waiting.state, KnowledgeAskState::Queued);
}
