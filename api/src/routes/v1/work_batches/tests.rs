// Copyright © 2026 Jalapeno Labs

//! The work queue's and work batches' routes driven through the router (#84): the queue in order,
//! a member running a batch next, pausing and cancelling it with each change in its history,
//! refusals once a batch moved on, an admin sizing the pool, and guests refused everything.
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
use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database, signed_in_cookie};

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

async fn submitted(connection: &mut AsyncPgConnection, project_id: Uuid, title: &str) -> Uuid {
    work_batch::submit(
        connection,
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
            created_by: PROJECT_MANAGER_USER_ID,
        },
    )
    .await
    .expect("submitted")
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
    let launch = project::create(
        &mut connection,
        &NewProject {
            name: "Launch".to_owned(),
            description: String::new(),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("a project")
    .id;
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
    assert_eq!(kinds, ["submitted", "paused", "cancelled"]);
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

    set_role(&mut connection, "guest").await;
    for (method, path) in [
        (Method::GET, "/api/v1/work-queue".to_owned()),
        (
            Method::GET,
            format!("/api/v1/work-batches/{}", Uuid::now_v7()),
        ),
    ] {
        let (status, body) = call(&state, method, &path, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
    }
}
