// Copyright © 2026 Jalapeno Labs

//! The findings routes driven through the router: a project's open findings with their count, a
//! member accepting and dismissing them, a second decision refused, and guests refused all three.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::models::finding::{self, Cost, NewFinding, Reporter};
use crate::models::project::{self, NewProject};
use crate::models::user::PROJECT_MANAGER_USER_ID;
use crate::models::work_batch::{NewWorkBatch, Priority};
use crate::routes;
use crate::test_support::{
    TEST_PERSON_ID, app_state, approved_batch, migrated_database, person_rooted, signed_in_cookie,
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

/// `title`, reported on a new batch's worker of `project_id`.
async fn reported(connection: &mut AsyncPgConnection, project_id: Uuid, title: &str) -> Uuid {
    let batch = approved_batch(
        connection,
        NewWorkBatch {
            id: Uuid::now_v7(),
            project_id,
            title: "Annual pricing".to_owned(),
            goal: "Show annual pricing on the pricing page.".to_owned(),
            repositories: json!([]),
            priority: Priority::Normal,
            scheduled_for: None,
            turn_id: None,
            source_record_ids: json!([]),
            action_item_ids: json!([]),
            source_message_ids: json!([]),
            serves: None,
            lineage: person_rooted(),
            created_by: PROJECT_MANAGER_USER_ID,
        },
    )
    .await;
    finding::report(
        connection,
        project_id,
        Reporter::Worker { batch_id: batch.id },
        NewFinding {
            title: title.to_owned(),
            detail: "Seen while working on annual pricing.".to_owned(),
            repository_url: None,
            location: "src/pricing.rs".to_owned(),
            cost: Cost::Medium,
        },
        chrono::Utc::now(),
    )
    .await
    .expect("reported")
    .finding
    .id
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_member_lists_accepts_and_dismisses_a_projects_findings() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let project_id = project::create(
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
    let first = reported(&mut connection, project_id, "Retry forever").await;
    let second = reported(&mut connection, project_id, "No docs for pricing").await;

    let list = format!("/api/v1/projects/{project_id}/findings");
    let (status, listed) = call(&state, Method::GET, &list, None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["total"], 2);
    assert_eq!(listed["findings"][0]["id"], json!(second), "newest first");
    assert_eq!(listed["findings"][0]["cost"], "medium");
    assert_eq!(listed["findings"][0]["state"], "open");

    let (status, accepted) = call(
        &state,
        Method::POST,
        &format!("/api/v1/findings/{first}/accept"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert_eq!(accepted["finding"]["state"], "accepted");
    assert!(accepted["itemId"].is_string());
    assert!(accepted["batchId"].is_string());
    assert_eq!(
        accepted["issue"],
        Value::Null,
        "the project names no tracker"
    );
    assert!(accepted["issueRefusal"].is_string());

    let (status, refused) = call(
        &state,
        Method::POST,
        &format!("/api/v1/findings/{first}/dismiss"),
        Some(json!({ "reason": "Changed my mind." })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "finding_decided");

    let (status, dismissed) = call(
        &state,
        Method::POST,
        &format!("/api/v1/findings/{second}/dismiss"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{dismissed}");
    assert_eq!(dismissed["finding"]["state"], "dismissed");

    let (status, listed) = call(&state, Method::GET, &list, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["total"], 0);
    assert_eq!(listed["findings"], json!([]));

    let (status, _) = call(
        &state,
        Method::POST,
        &format!("/api/v1/findings/{}/accept", Uuid::now_v7()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &state,
        Method::GET,
        &format!("/api/v1/projects/{}/findings", Uuid::now_v7()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_guest_reads_and_decides_no_finding() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let project_id = project::create(
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
    let id = reported(&mut connection, project_id, "Retry forever").await;
    diesel::sql_query(format!(
        "UPDATE users SET role = 'guest' WHERE id = '{TEST_PERSON_ID}'"
    ))
    .execute(&mut connection)
    .await
    .expect("the role changes");

    for (method, path, body) in [
        (
            Method::GET,
            format!("/api/v1/projects/{project_id}/findings"),
            None,
        ),
        (Method::POST, format!("/api/v1/findings/{id}/accept"), None),
        (
            Method::POST,
            format!("/api/v1/findings/{id}/dismiss"),
            Some(json!({})),
        ),
    ] {
        let (status, body) = call(&state, method, &path, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
    }
    assert_eq!(
        finding::count_open(&mut connection, project_id)
            .await
            .expect("counted"),
        1,
        "nothing was decided"
    );
}
