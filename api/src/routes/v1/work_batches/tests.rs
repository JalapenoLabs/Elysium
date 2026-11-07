// Copyright © 2026 Jalapeno Labs

//! The work queue's and work batches' routes driven through the router (#84): the queue in order,
//! a member running a batch next, pausing and cancelling it with each change in its history,
//! refusals once a batch moved on, a member approving, editing, and rejecting what was proposed
//! (#211), an admin sizing the pool, and guests refused everything.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::models::project::{self, NewProject};
use crate::models::user::PROJECT_MANAGER_USER_ID;
use crate::models::work_batch::{self, NewWorkBatch, Priority};
use crate::routes;
use crate::test_support::{
    TEST_PERSON_ID, app_state, approved_batch, migrated_database, signed_in_cookie,
};

/// A router test's answer to `method path` with `body`, as the test person.
async fn call(
    state: &crate::state::AppState,
    method: Method,
    path: &str,
    body: Option<Value>,
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
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("the body reads");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
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
        lineage: crate::test_support::person_rooted(),
        created_by: PROJECT_MANAGER_USER_ID,
    }
}

/// A batch a person approved, waiting for a slot.
async fn submitted(connection: &mut AsyncPgConnection, project_id: Uuid, title: &str) -> Uuid {
    approved_batch(connection, new_batch(project_id, title))
        .await
        .id
}

/// A batch its Project Manager proposed, waiting for a person.
async fn proposed(connection: &mut AsyncPgConnection, project_id: Uuid, title: &str) -> Uuid {
    work_batch::submit(connection, new_batch(project_id, title))
        .await
        .expect("submitted")
        .batch
        .id
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

async fn set_role(connection: &mut AsyncPgConnection, role: &str) {
    diesel::sql_query(format!(
        "UPDATE users SET role = '{role}' WHERE id = '{TEST_PERSON_ID}'"
    ))
    .execute(connection)
    .await
    .expect("the role changes");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_member_reorders_pauses_and_cancels_what_waits() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let launch = project_named(&mut connection, "Launch").await;
    let first = submitted(&mut connection, launch, "Pricing page").await;
    let second = submitted(&mut connection, launch, "Docs").await;

    let (status, queue) = call(&state, Method::GET, "/api/v1/work-queue", None).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    assert_eq!(queue["workQueue"]["next"][0]["id"], json!(first));
    assert_eq!(queue["workQueue"]["next"][1]["id"], json!(second));
    assert_eq!(
        queue["workQueue"]["pool"]["size"],
        Value::Null,
        "no engine has looked, and no admin chose a size"
    );

    let (status, moved) = call(
        &state,
        Method::PATCH,
        &format!("/api/v1/work-batches/{second}"),
        Some(json!({ "runNext": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert!(moved["batch"]["pinnedAt"].is_string());
    let (_, queue) = call(&state, Method::GET, "/api/v1/work-queue", None).await;
    assert_eq!(queue["workQueue"]["next"][0]["id"], json!(second));

    let (status, paused) = call(
        &state,
        Method::PATCH,
        &format!("/api/v1/work-batches/{first}"),
        Some(json!({ "paused": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{paused}");
    assert_eq!(paused["batch"]["state"], "paused");
    let (_, queue) = call(&state, Method::GET, "/api/v1/work-queue", None).await;
    assert_eq!(queue["workQueue"]["paused"][0]["id"], json!(first));

    let (status, cancelled) = call(
        &state,
        Method::POST,
        &format!("/api/v1/work-batches/{first}/cancel"),
        Some(json!({ "reason": "Not this quarter." })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cancelled}");
    assert_eq!(cancelled["batch"]["result"], "Not this quarter.");
    let kinds: Vec<&str> = cancelled["history"]
        .as_array()
        .expect("history")
        .iter()
        .filter_map(|entry| entry["kind"].as_str())
        .collect();
    assert_eq!(kinds, ["submitted", "approved", "paused", "cancelled"]);
    assert_eq!(cancelled["history"][1]["createdBy"], json!(TEST_PERSON_ID));

    let (status, refused) = call(
        &state,
        Method::PATCH,
        &format!("/api/v1/work-batches/{first}"),
        Some(json!({ "paused": false })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "work_batch_moved_on");

    let (status, _invalid) = call(
        &state,
        Method::PATCH,
        &format!("/api/v1/work-batches/{second}"),
        Some(json!({ "priority": "scheduled", "scheduledFor": "2020-01-01T00:00:00Z" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _missing) = call(
        &state,
        Method::GET,
        &format!("/api/v1/work-batches/{}", Uuid::now_v7()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_member_approves_a_proposal_as_it_is_or_edited() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let launch = project_named(&mut connection, "Launch").await;
    let pricing = proposed(&mut connection, launch, "Pricing page").await;
    let docs = proposed(&mut connection, launch, "Docs").await;

    let (status, queue) = call(&state, Method::GET, "/api/v1/work-queue", None).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    assert_eq!(queue["workQueue"]["proposed"][0]["id"], json!(pricing));
    assert_eq!(queue["workQueue"]["proposed"][1]["id"], json!(docs));
    assert_eq!(
        queue["workQueue"]["next"],
        json!([]),
        "nothing proposed is in line for a slot"
    );
    let (_, manager) = call(
        &state,
        Method::GET,
        &format!("/api/v1/projects/{launch}/project-manager"),
        None,
    )
    .await;
    assert_eq!(
        manager["projectManager"]["proposed"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );

    // A proposal is approved or rejected, never resumed into the queue.
    let (status, refused) = call(
        &state,
        Method::PATCH,
        &format!("/api/v1/work-batches/{pricing}"),
        Some(json!({ "paused": false })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");

    let (status, approved) = call(
        &state,
        Method::POST,
        &format!("/api/v1/work-batches/{pricing}/approve"),
        Some(json!({ "title": "  Annual pricing page  ", "priority": "asap" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    assert_eq!(approved["batch"]["state"], "queued");
    assert_eq!(
        approved["batch"]["title"], "Annual pricing page",
        "tidied as a submission is"
    );
    assert_eq!(approved["batch"]["priority"], "asap");
    assert_eq!(
        approved["why"],
        json!({ "records": [], "messages": [], "actionItems": [], "parents": [] })
    );
    let kinds: Vec<&str> = approved["history"]
        .as_array()
        .expect("history")
        .iter()
        .filter_map(|entry| entry["kind"].as_str())
        .collect();
    assert_eq!(kinds, ["submitted", "edited", "approved"]);
    assert_eq!(approved["history"][2]["createdBy"], json!(TEST_PERSON_ID));
    let (_, queue) = call(&state, Method::GET, "/api/v1/work-queue", None).await;
    assert_eq!(queue["workQueue"]["next"][0]["id"], json!(pricing));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_member_rejects_a_proposal_saying_why() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let launch = project_named(&mut connection, "Launch").await;
    let docs = proposed(&mut connection, launch, "Docs").await;

    let (status, _blank) = call(
        &state,
        Method::POST,
        &format!("/api/v1/work-batches/{docs}/reject"),
        Some(json!({ "reason": "   " })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a rejection says why");
    let (status, _invalid) = call(
        &state,
        Method::POST,
        &format!("/api/v1/work-batches/{docs}/approve"),
        Some(json!({ "repositories": [{ "url": "not a repository" }] })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an edit is held to a batch's rules"
    );
    let (status, rejected) = call(
        &state,
        Method::POST,
        &format!("/api/v1/work-batches/{docs}/reject"),
        Some(json!({ "reason": "Docs wait for the redesign." })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rejected}");
    assert_eq!(rejected["batch"]["state"], "rejected");
    assert_eq!(rejected["batch"]["result"], "Docs wait for the redesign.");

    let (status, moved_on) = call(
        &state,
        Method::POST,
        &format!("/api/v1/work-batches/{docs}/approve"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{moved_on}");
    assert_eq!(moved_on["code"], "work_batch_moved_on");
    let (_, queue) = call(&state, Method::GET, "/api/v1/work-queue", None).await;
    assert_eq!(queue["workQueue"]["proposed"], json!([]));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn an_admin_sizes_the_pool_and_a_guest_reaches_nothing() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;

    let (status, settings) = call(
        &state,
        Method::PATCH,
        "/api/v1/workspace-settings",
        Some(json!({ "workerPoolSize": 12 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{settings}");
    assert_eq!(settings["settings"]["workerPoolSize"], 12);
    let (_, queue) = call(&state, Method::GET, "/api/v1/work-queue", None).await;
    assert_eq!(queue["workQueue"]["pool"]["size"], 12);
    assert_eq!(queue["workQueue"]["pool"]["configuredSize"], 12);
    let (status, _refused) = call(
        &state,
        Method::PATCH,
        "/api/v1/workspace-settings",
        Some(json!({ "workerPoolSize": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "at least one worker");
    let (_, settings) = call(
        &state,
        Method::PATCH,
        "/api/v1/workspace-settings",
        Some(json!({ "workerPoolSize": null })),
    )
    .await;
    assert_eq!(settings["settings"]["workerPoolSize"], Value::Null);

    let launch = project_named(&mut connection, "Launch").await;
    let waiting = proposed(&mut connection, launch, "Pricing page").await;
    set_role(&mut connection, "guest").await;
    for (method, path, body) in [
        (Method::GET, "/api/v1/work-queue".to_owned(), None),
        (
            Method::GET,
            format!("/api/v1/work-batches/{}", Uuid::now_v7()),
            None,
        ),
        (
            Method::POST,
            format!("/api/v1/work-batches/{waiting}/approve"),
            Some(json!({})),
        ),
        (
            Method::POST,
            format!("/api/v1/work-batches/{waiting}/reject"),
            Some(json!({ "reason": "No." })),
        ),
    ] {
        let (status, body) = call(&state, method, &path, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
    }
}
