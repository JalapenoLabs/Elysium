// Copyright © 2026 Jalapeno Labs

//! The runs routes driven through the router (#75): a page of runs, one run with its steps and
//! history, a step's log, a person's retry recorded with who did it, refusals for actions the
//! step's state does not allow, and guests refused everything.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::Utc;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::models::step_job::{self, StepOutcome};
use crate::models::user::SYSTEM_USER_ID;
use crate::models::workflow_run::{self, NewRun};
use crate::routes;
use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database, signed_in_cookie};

/// A router test's answer to `method path`, as the test person.
async fn call(state: &crate::state::AppState, method: Method, path: &str) -> (StatusCode, Value) {
    let cookie = signed_in_cookie(state).await;
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::COOKIE, cookie)
        .header(header::ORIGIN, "http://localhost:4000")
        .body(Body::empty())
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

/// Starts a one-step run on `source_id` and fails its step for good, as the engine would.
async fn failed_run(connection: &mut AsyncPgConnection, source_id: &str) -> (Uuid, Uuid) {
    let started = workflow_run::start(
        connection,
        NewRun {
            workflow: "test".to_owned(),
            source_kind: "test_source".to_owned(),
            source_id: source_id.to_owned(),
            input: json!({}),
            steps: vec!["fetch".to_owned()],
            created_by: SYSTEM_USER_ID,
        },
        Utc::now(),
    )
    .await
    .expect("the run starts");
    let at = Utc::now();
    let claims = step_job::claim(connection, at, 1)
        .await
        .expect("claims")
        .claims;
    step_job::append_log(connection, &claims[0], "fetching\n")
        .await
        .expect("logs");
    step_job::finish(
        connection,
        &claims[0],
        StepOutcome::Failed {
            reason: "the source is gone".to_owned(),
            retryable: false,
        },
        at,
    )
    .await
    .expect("records");
    (started.settled.run.id, started.settled.steps[0].id)
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
async fn a_member_reads_runs_and_retries_a_failed_step() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let (run_id, step_id) = failed_run(&mut connection, "m-1").await;

    let (status, page) = call(&state, Method::GET, "/api/v1/runs").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["runs"][0]["id"], json!(run_id));
    assert_eq!(page["runs"][0]["state"], "failed");
    assert_eq!(page["runs"][0]["error"], "the source is gone");
    assert_eq!(page["hasMore"], false);
    let (_, older) = call(
        &state,
        Method::GET,
        &format!("/api/v1/runs?before={run_id}"),
    )
    .await;
    assert_eq!(
        older["runs"],
        json!([]),
        "nothing is older than the oldest run"
    );

    let (status, run) = call(&state, Method::GET, &format!("/api/v1/runs/{run_id}")).await;
    assert_eq!(status, StatusCode::OK, "{run}");
    assert_eq!(run["steps"][0]["step"], "fetch");
    assert_eq!(run["steps"][0]["state"], "failed");
    assert!(
        run["steps"][0].get("input").is_none(),
        "content stays off the run page's read"
    );
    assert_eq!(run["history"][0]["kind"], "started");

    let log_path = format!("/api/v1/runs/{run_id}/steps/{step_id}/log");
    let (status, log) = call(&state, Method::GET, &log_path).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        log["log"],
        "fetching\nAttempt 1 failed: the source is gone\n"
    );

    let skip_path = format!("/api/v1/runs/{run_id}/steps/{step_id}/resume");
    let (status, refused) = call(&state, Method::POST, &skip_path).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "only a held step resumes: {refused}"
    );

    let retry_path = format!("/api/v1/runs/{run_id}/steps/{step_id}/retry");
    let (status, retried) = call(&state, Method::POST, &retry_path).await;
    assert_eq!(status, StatusCode::OK, "{retried}");
    assert_eq!(retried["run"]["state"], "running");
    assert_eq!(retried["steps"][0]["state"], "queued");
    assert_eq!(retried["history"][1]["kind"], "retried");
    assert_eq!(retried["history"][1]["createdBy"], json!(TEST_PERSON_ID));

    let (status, _) = call(&state, Method::POST, &retry_path).await;
    assert_eq!(status, StatusCode::CONFLICT, "a queued step is not retried");
    let elsewhere = format!("/api/v1/runs/{}/steps/{step_id}/retry", Uuid::now_v7());
    let (status, _) = call(&state, Method::POST, &elsewhere).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a step is reached through its own run"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_guest_reaches_no_run() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let (run_id, step_id) = failed_run(&mut connection, "m-1").await;
    set_role(&mut connection, "guest").await;

    for (method, path) in [
        (Method::GET, "/api/v1/runs".to_owned()),
        (Method::GET, format!("/api/v1/runs/{run_id}")),
        (
            Method::GET,
            format!("/api/v1/runs/{run_id}/steps/{step_id}/log"),
        ),
        (
            Method::POST,
            format!("/api/v1/runs/{run_id}/steps/{step_id}/retry"),
        ),
        (
            Method::POST,
            format!("/api/v1/runs/{run_id}/steps/{step_id}/skip"),
        ),
        (
            Method::POST,
            format!("/api/v1/runs/{run_id}/steps/{step_id}/resume"),
        ),
    ] {
        let (status, refused) = call(&state, method.clone(), &path).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
        assert_eq!(refused["code"], "members_only", "{method} {path}");
    }
    let steps = workflow_run::steps_of(&mut connection, run_id)
        .await
        .expect("steps read");
    assert_eq!(
        steps[0].state,
        step_job::StepJobState::Failed,
        "nothing a guest sent changed the run"
    );
}
