// Copyright © 2026 Jalapeno Labs

//! Model settings against Postgres (#90): every step and agent listed with its default, an
//! admin's choice stored, recorded, and resolved by the turns that read it, a choice equal to
//! the default stored as none, a step that runs no model and a flag-like model refused, and
//! nobody but admins reaching any of it.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use diesel::QueryableByName;
use diesel::sql_types::{Jsonb, Text};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use tower::ServiceExt as _;

use crate::agent_models::{self, Agent};
use crate::models::model_setting::{Effort, ModelChoice};
use crate::routes;
use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database, signed_in_cookie};

#[derive(QueryableByName)]
struct Event {
    #[diesel(sql_type = Text)]
    kind: String,
    #[diesel(sql_type = Jsonb)]
    data: Value,
}

/// The model settings changes the workspace's history recorded, oldest first.
async fn setting_events(connection: &mut AsyncPgConnection) -> Vec<Event> {
    diesel::sql_query(
        "SELECT kind, data FROM user_events \
         WHERE user_id IS NULL AND kind = 'model_setting_changed' ORDER BY id",
    )
    .load(connection)
    .await
    .expect("the events load")
}

/// A router test's answer to `method path` with `body`, as the test person.
async fn call(
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
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("the body reads");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// The listed entry named `name`.
fn listed<'a>(list: &'a Value, name: &str) -> &'a Value {
    list["agents"]
        .as_array()
        .and_then(|agents| agents.iter().find(|agent| agent["name"] == name))
        .unwrap_or_else(|| panic!("{name} is listed: {list}"))
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn an_admins_choice_is_what_the_next_turn_runs_on_until_put_back_on_its_default() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;

    let (status, list) = call(&state, Method::GET, "/api/v1/model-settings", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(
        listed(&list, "elysia")["default"],
        json!({ "model": "claude-opus-5-5", "effort": "high" })
    );
    assert_eq!(listed(&list, "elysia")["setting"], Value::Null);
    assert_eq!(listed(&list, "elysia")["kind"], "agent");
    assert_eq!(
        listed(&list, "turn_summary")["default"],
        json!({ "model": "claude-haiku-5-5", "effort": "low" })
    );
    assert_eq!(listed(&list, "transcribe_meeting")["kind"], "step");
    assert_eq!(
        listed(&list, "transcribe_meeting")["default"],
        Value::Null,
        "Whisper runs no model of Claude's"
    );
    assert!(
        list["suggestedModels"]
            .as_array()
            .expect("suggestions")
            .contains(&json!("claude-opus-5-5"))
    );
    assert_eq!(
        list["efforts"],
        json!(["low", "medium", "high", "xhigh", "max"])
    );

    let sonnet = json!({ "model": " claude-sonnet-5-5 ", "effort": "medium" });
    let (status, set) = call(
        &state,
        Method::PUT,
        "/api/v1/model-settings/elysia",
        Some(&sonnet),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    assert_eq!(
        set["agent"]["setting"]["model"], "claude-sonnet-5-5",
        "trimmed"
    );
    assert_eq!(set["agent"]["setting"]["effort"], "medium");
    assert_eq!(
        agent_models::for_agent(&mut connection, Agent::Elysia)
            .await
            .expect("resolves"),
        ModelChoice {
            model: "claude-sonnet-5-5".to_owned(),
            effort: Effort::Medium,
        },
        "her next turn runs on it"
    );
    assert_eq!(
        agent_models::for_agent(&mut connection, Agent::CodingSession)
            .await
            .expect("resolves"),
        ModelChoice::default(),
        "every other agent keeps its default"
    );

    // Choosing it again changes and records nothing.
    call(
        &state,
        Method::PUT,
        "/api/v1/model-settings/elysia",
        Some(&sonnet),
    )
    .await;
    let events = setting_events(&mut connection).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "model_setting_changed");
    assert_eq!(
        events[0].data,
        json!({
            "agent": "elysia",
            "before": null,
            "after": { "model": "claude-sonnet-5-5", "effort": "medium" },
        })
    );

    // Choosing the default stores no setting, so a later default applies.
    let (status, defaulted) = call(
        &state,
        Method::PUT,
        "/api/v1/model-settings/elysia",
        Some(&json!({ "model": "claude-opus-5-5", "effort": "high" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{defaulted}");
    assert_eq!(defaulted["agent"]["setting"], Value::Null);
    let events = setting_events(&mut connection).await;
    assert_eq!(
        events.last().expect("recorded").data["after"],
        Value::Null,
        "the history says it went back to its default"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn an_alias_the_cli_takes_is_a_model_and_delete_puts_an_agent_back_on_its_default() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;

    let (status, _) = call(
        &state,
        Method::PUT,
        "/api/v1/model-settings/turn_summary",
        Some(&json!({ "model": "opus[1m]", "effort": "max" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "an alias the CLI takes is a model");
    let (status, reset) = call(
        &state,
        Method::DELETE,
        "/api/v1/model-settings/turn_summary",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reset}");
    assert_eq!(reset["agent"]["setting"], Value::Null);
    assert_eq!(
        agent_models::for_agent(&mut connection, Agent::TurnSummary)
            .await
            .expect("resolves"),
        Agent::TurnSummary.default_choice()
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_flag_a_step_without_a_model_and_a_name_nothing_has_are_refused() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;

    for (path, body, expected) in [
        (
            "/api/v1/model-settings/elysia",
            json!({ "model": "--dangerously-skip-permissions", "effort": "high" }),
            StatusCode::BAD_REQUEST,
        ),
        (
            "/api/v1/model-settings/elysia",
            json!({ "model": "opus", "effort": "extreme" }),
            StatusCode::BAD_REQUEST,
        ),
        (
            "/api/v1/model-settings/transcribe_meeting",
            json!({ "model": "claude-opus-5-5", "effort": "low" }),
            StatusCode::CONFLICT,
        ),
        (
            "/api/v1/model-settings/correct_spelling",
            json!({ "model": "claude-opus-5-5", "effort": "low" }),
            StatusCode::NOT_FOUND,
        ),
    ] {
        let (status, answer) = call(&state, Method::PUT, path, Some(&body)).await;
        assert_eq!(status, expected, "{path} {body}: {answer}");
    }
    assert!(
        setting_events(&mut connection).await.is_empty(),
        "nothing refused was recorded"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn only_admins_read_or_change_model_settings() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    diesel::sql_query(format!(
        "UPDATE users SET role = 'member' WHERE id = '{TEST_PERSON_ID}'"
    ))
    .execute(&mut connection)
    .await
    .expect("the role changes");

    let choice = json!({ "model": "claude-haiku-5-5", "effort": "low" });
    for (method, body) in [
        (Method::GET, None),
        (Method::PUT, Some(&choice)),
        (Method::DELETE, None),
    ] {
        let path = if method == Method::GET {
            "/api/v1/model-settings"
        } else {
            "/api/v1/model-settings/elysia"
        };
        let (status, refused) = call(&state, method.clone(), path, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
        assert_eq!(refused["code"], "admin_only", "{method} {path}");
    }
}
