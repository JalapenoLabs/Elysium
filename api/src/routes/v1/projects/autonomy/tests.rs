// Copyright © 2026 Jalapeno Labs

//! The trust dial's routes driven through the router: a member turns it and the project answers
//! the level with who set it and when; a guest reads it but cannot turn it; an unknown level is
//! refused.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::models::project::{self, NewProject};
use crate::routes;
use crate::state::AppState;
use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database, signed_in_cookie};

/// A router test's answer to `method path` with `body`, as the test person.
async fn call(
    state: &AppState,
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
async fn a_member_turns_the_dial_and_the_project_says_who_set_it() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let launch = project_named(&mut connection, "Launch").await;
    let path = format!("/api/v1/projects/{launch}/autonomy");

    let (status, fresh) = call(&state, Method::GET, &path, None).await;
    assert_eq!(status, StatusCode::OK, "{fresh}");
    assert_eq!(
        fresh["autonomy"],
        json!({ "autonomy": "approve_every_batch", "setBy": null, "setAt": null })
    );

    let (status, turned) = call(
        &state,
        Method::PUT,
        &path,
        Some(json!({ "autonomy": "approve_found_work" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{turned}");
    assert_eq!(turned["project"]["autonomy"], "approve_found_work");
    assert_eq!(turned["autonomy"]["autonomy"], "approve_found_work");
    assert_eq!(turned["autonomy"]["setBy"], json!(TEST_PERSON_ID));
    assert!(turned["autonomy"]["setAt"].is_string());

    let (_, projects) = call(&state, Method::GET, "/api/v1/projects", None).await;
    assert_eq!(projects["projects"][0]["autonomy"], "approve_found_work");

    let (status, refused) = call(
        &state,
        Method::PUT,
        &path,
        Some(json!({ "autonomy": "anything_goes" })),
    )
    .await;
    assert!(status.is_client_error(), "{status}: {refused}");
    let (status, refused) = call(
        &state,
        Method::PUT,
        &path,
        Some(json!({ "autonomy": "full_autonomy", "setBy": TEST_PERSON_ID })),
    )
    .await;
    assert!(
        status.is_client_error(),
        "who set it is never taken: {refused}"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_guest_reads_the_dial_but_cannot_turn_it() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let launch = project_named(&mut connection, "Launch").await;
    let path = format!("/api/v1/projects/{launch}/autonomy");
    set_role(&mut connection, "guest").await;

    let (status, read) = call(&state, Method::GET, &path, None).await;
    assert_eq!(status, StatusCode::OK, "{read}");
    let (status, refused) = call(
        &state,
        Method::PUT,
        &path,
        Some(json!({ "autonomy": "full_autonomy" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    let unchanged = project::find(&mut connection, launch).await.expect("read");
    assert_eq!(
        unchanged.autonomy,
        crate::models::project_autonomy::Autonomy::ApproveEveryBatch
    );
}
