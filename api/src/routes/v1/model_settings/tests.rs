// Copyright © 2026 Jalapeno Labs

//! Model settings against Postgres (#90, #146): every kind, step, and agent listed with what it
//! runs on and why; an admin's choice for an agent or a kind, and a project's for its own
//! agents, stored, recorded, and resolved by the turns that read them in the documented order;
//! a choice equal to what it follows stored as none; a step that runs no model, a kind that
//! runs in no project, and a flag-like model refused; what a kind's turns spent totalled; and
//! nobody but admins reaching the workspace's settings.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::Utc;
use diesel::QueryableByName;
use diesel::sql_types::{Jsonb, Text};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::agent_models::{self, Agent, AgentKind, Source};
use crate::models::coding_session::{self, NewCodingSession};
use crate::models::model_setting::{Effort, ModelChoice};
use crate::models::project::{self, NewProject};
use crate::models::session_event::{self, StoredEvent};
use crate::routes;
use crate::runner::{RunnerEvent, TurnOutcome, TurnStatus};
use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database, signed_in_cookie};

#[derive(QueryableByName)]
struct Event {
    #[diesel(sql_type = Jsonb)]
    data: Value,
}

/// The workspace's history of model setting changes of `kind`, oldest first.
async fn setting_events(connection: &mut AsyncPgConnection, kind: &str) -> Vec<Event> {
    diesel::sql_query(
        "SELECT data FROM user_events \
         WHERE user_id IS NULL AND kind = $1 ORDER BY id",
    )
    .bind::<Text, _>(kind)
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

/// The listed entry of `list[field]` named `name`.
fn listed<'a>(list: &'a Value, field: &str, name: &str) -> &'a Value {
    list[field]
        .as_array()
        .and_then(|entries| entries.iter().find(|entry| entry["name"] == name))
        .unwrap_or_else(|| panic!("{name} is listed in {field}: {list}"))
}

fn choice(model: &str, effort: Effort) -> ModelChoice {
    ModelChoice {
        model: model.to_owned(),
        effort,
    }
}

async fn new_project(connection: &mut AsyncPgConnection, name: &str) -> Uuid {
    project::create(
        connection,
        &NewProject {
            name: name.to_owned(),
            description: String::new(),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("the project is created")
    .id
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn the_list_names_every_kind_step_and_agent_with_what_it_runs_on_and_why() {
    let (url, _connection) = migrated_database().await;
    let state = app_state(&url).await;

    let (status, list) = call(&state, Method::GET, "/api/v1/model-settings", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");

    let kinds: Vec<&Value> = list["kinds"]
        .as_array()
        .expect("kinds")
        .iter()
        .map(|kind| &kind["name"])
        .collect();
    assert_eq!(
        kinds,
        [
            "worker",
            "project_manager",
            "correction",
            "segmenting",
            "triage",
            "elysia",
            "summaries"
        ]
    );
    let summaries = listed(&list, "kinds", "summaries");
    assert_eq!(
        summaries["default"],
        json!({ "model": "claude-haiku-5-5", "effort": "low" })
    );
    assert_eq!(summaries["runsInProjects"], true);
    assert_eq!(summaries["usage"], Value::Null, "summaries keep no cost");
    assert_eq!(
        listed(&list, "kinds", "worker")["usage"],
        json!({ "turns": 0, "costUsdMicros": 0, "inputTokens": 0, "outputTokens": 0 })
    );
    assert_eq!(listed(&list, "kinds", "elysia")["runsInProjects"], false);

    let elysia = listed(&list, "agents", "elysia");
    assert_eq!(elysia["isStep"], false);
    assert_eq!(elysia["kind"], "elysia");
    assert_eq!(elysia["setting"], Value::Null);
    assert_eq!(
        elysia["resolved"],
        json!({ "model": "claude-opus-5-5", "effort": "high", "source": "default" })
    );
    assert_eq!(listed(&list, "agents", "coding_session")["kind"], "worker");
    let whisper = listed(&list, "agents", "transcribe_meeting");
    assert_eq!(whisper["isStep"], true);
    assert_eq!(
        whisper["kind"],
        Value::Null,
        "Whisper runs no model of Claude's"
    );
    assert_eq!(whisper["resolved"], Value::Null);
    assert_eq!(
        listed(&list, "agents", "review_company_doc")["kind"],
        "elysia"
    );

    assert!(
        list["choices"]["suggestedModels"]
            .as_array()
            .expect("suggestions")
            .contains(&json!("claude-opus-5-5"))
    );
    assert_eq!(
        list["choices"]["modelPrices"][1],
        json!({
            "model": "claude-opus-5-5",
            "inputCentsPerMillionTokens": 400,
            "outputCentsPerMillionTokens": 2000,
        })
    );
    assert_eq!(
        list["choices"]["efforts"],
        json!(["low", "medium", "high", "xhigh", "max"])
    );
    assert!(list["usageSince"].is_string());
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn an_admins_choice_is_what_the_next_turn_runs_on_until_put_back() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;

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
    assert_eq!(set["agent"]["resolved"]["source"], "agent");
    assert_eq!(
        agent_models::for_agent(&mut connection, Agent::Elysia, None)
            .await
            .expect("resolves")
            .choice,
        choice("claude-sonnet-5-5", Effort::Medium),
        "her next turn runs on it"
    );
    assert_eq!(
        agent_models::for_agent(&mut connection, Agent::CodingSession, None)
            .await
            .expect("resolves")
            .choice,
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
    let events = setting_events(&mut connection, "model_setting_changed").await;
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].data,
        json!({
            "agent": "elysia",
            "before": null,
            "after": { "model": "claude-sonnet-5-5", "effort": "medium" },
        })
    );

    // Choosing what it would follow anyway stores no setting, so a later change of the kind
    // reaches it.
    let (status, defaulted) = call(
        &state,
        Method::PUT,
        "/api/v1/model-settings/elysia",
        Some(&json!({ "model": "claude-opus-5-5", "effort": "high" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{defaulted}");
    assert_eq!(defaulted["agent"]["setting"], Value::Null);
    let events = setting_events(&mut connection, "model_setting_changed").await;
    assert_eq!(
        events.last().expect("recorded").data["after"],
        Value::Null,
        "the history says it went back to following its kind"
    );

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
        agent_models::for_agent(&mut connection, Agent::TurnSummary, None)
            .await
            .expect("resolves")
            .choice,
        AgentKind::Summaries.default_choice()
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_turn_runs_on_its_projects_choice_then_its_own_then_its_kinds_then_the_default() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let project = new_project(&mut connection, "Elysium").await;
    let other = new_project(&mut connection, "Seraphim").await;
    let resolve =
        async |connection: &mut AsyncPgConnection, agent: Agent, project: Option<Uuid>| {
            agent_models::for_agent(connection, agent, project)
                .await
                .expect("resolves")
        };

    // 4. Nothing chosen: the kind's default.
    let resolved = resolve(&mut connection, Agent::CodingSession, Some(project)).await;
    assert_eq!(resolved.source, Source::Default);

    // 3. The kind, for every worker without a setting of its own.
    let (status, kind) = call(
        &state,
        Method::PUT,
        "/api/v1/model-settings/kinds/worker",
        Some(&json!({ "model": "claude-sonnet-5-5", "effort": "medium" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{kind}");
    assert_eq!(kind["kind"]["setting"]["model"], "claude-sonnet-5-5");
    for agent in [Agent::CodingSession, Agent::StudioSession] {
        let resolved = resolve(&mut connection, agent, Some(project)).await;
        assert_eq!(
            (resolved.choice, resolved.source),
            (choice("claude-sonnet-5-5", Effort::Medium), Source::Kind),
            "{agent:?}"
        );
    }

    // 2. One agent's own setting stays the exception to its kind.
    call(
        &state,
        Method::PUT,
        "/api/v1/model-settings/studio_session",
        Some(&json!({ "model": "claude-fable-5-1", "effort": "xhigh" })),
    )
    .await;
    let studio = resolve(&mut connection, Agent::StudioSession, Some(project)).await;
    assert_eq!(
        (studio.choice, studio.source),
        (choice("claude-fable-5-1", Effort::Xhigh), Source::Agent)
    );

    // 1. A project's choice wins for that project's agents alone.
    let (status, overridden) = call(
        &state,
        Method::PUT,
        &format!("/api/v1/projects/{project}/model-settings/worker"),
        Some(&json!({ "model": "claude-haiku-5-5", "effort": "low" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{overridden}");
    assert_eq!(overridden["kind"]["setting"]["model"], "claude-haiku-5-5");
    assert_eq!(
        overridden["kind"]["workspace"],
        json!({ "model": "claude-sonnet-5-5", "effort": "medium", "source": "kind" })
    );
    for agent in [Agent::CodingSession, Agent::StudioSession] {
        let resolved = resolve(&mut connection, agent, Some(project)).await;
        assert_eq!(
            (resolved.choice, resolved.source),
            (choice("claude-haiku-5-5", Effort::Low), Source::Project),
            "{agent:?}"
        );
    }
    let elsewhere = resolve(&mut connection, Agent::CodingSession, Some(other)).await;
    assert_eq!(
        elsewhere.source,
        Source::Kind,
        "another project follows the workspace"
    );
    let unscoped = resolve(&mut connection, Agent::StudioSession, None).await;
    assert_eq!(
        unscoped.source,
        Source::Agent,
        "work for no project ignores overrides"
    );

    let history = setting_events(&mut connection, "kind_model_setting_changed").await;
    assert_eq!(
        history.last().expect("recorded").data,
        json!({
            "kind": "worker",
            "projectId": project,
            "before": null,
            "after": { "model": "claude-haiku-5-5", "effort": "low" },
        })
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_projects_choice_equal_to_the_workspaces_is_none_and_its_list_names_its_kinds() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let project = new_project(&mut connection, "Elysium").await;
    let worker = format!("/api/v1/projects/{project}/model-settings/worker");
    let sonnet = json!({ "model": "claude-sonnet-5-5", "effort": "medium" });
    call(
        &state,
        Method::PUT,
        "/api/v1/model-settings/kinds/worker",
        Some(&sonnet),
    )
    .await;
    call(
        &state,
        Method::PUT,
        &worker,
        Some(&json!({ "model": "claude-haiku-5-5", "effort": "low" })),
    )
    .await;

    // Choosing what the workspace runs anyway removes the project's choice.
    let (status, followed) = call(&state, Method::PUT, &worker, Some(&sonnet)).await;
    assert_eq!(status, StatusCode::OK, "{followed}");
    assert_eq!(followed["kind"]["setting"], Value::Null);
    let resolved = agent_models::for_agent(&mut connection, Agent::CodingSession, Some(project))
        .await
        .expect("resolves");
    assert_eq!(resolved.source, Source::Kind);

    // The project's list shows the kinds that work for it, and nothing else.
    let (status, list) = call(
        &state,
        Method::GET,
        &format!("/api/v1/projects/{project}/model-settings"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let names: Vec<&Value> = list["kinds"]
        .as_array()
        .expect("kinds")
        .iter()
        .map(|kind| &kind["name"])
        .collect();
    assert_eq!(names, ["worker", "project_manager", "summaries"]);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_kind_changed_again_is_changed_in_place_for_the_workspace_and_each_project() {
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        rows: i64,
    }

    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let project = new_project(&mut connection, "Elysium").await;
    let workspace = "/api/v1/model-settings/kinds/summaries".to_owned();
    let in_project = format!("/api/v1/projects/{project}/model-settings/summaries");

    // The workspace's row has no project, so only the unique constraint's nulls not distinct
    // keeps a second choice from adding a second row. The project's choices differ from the
    // workspace's, which would otherwise remove the project's row as one it follows anyway.
    for (path, models) in [
        (&workspace, ["claude-sonnet-5-5", "claude-fable-5-1"]),
        (&in_project, ["claude-opus-5-5", "claude-sonnet-5-5"]),
    ] {
        for model in models {
            let (status, answer) = call(
                &state,
                Method::PUT,
                path,
                Some(&json!({ "model": model, "effort": "medium" })),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{path}: {answer}");
        }
    }

    let count: Count = diesel::sql_query("SELECT COUNT(*) AS rows FROM kind_model_settings")
        .get_result(&mut connection)
        .await
        .expect("counted");
    assert_eq!(
        count.rows, 2,
        "one row for the workspace and one for the project"
    );
    let (_status, list) = call(&state, Method::GET, "/api/v1/model-settings", None).await;
    assert_eq!(
        listed(&list, "kinds", "summaries")["setting"]["model"],
        "claude-fable-5-1"
    );
    let history = setting_events(&mut connection, "kind_model_setting_changed").await;
    assert_eq!(history.len(), 4, "every change is recorded");
    assert_eq!(
        history[1].data["before"],
        json!({ "model": "claude-sonnet-5-5", "effort": "medium" })
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn what_a_projects_workers_spent_is_totalled_from_their_turns() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let project = new_project(&mut connection, "Elysium").await;
    let other = new_project(&mut connection, "Seraphim").await;
    for (owner, cost) in [(project, 120_000), (project, 30_000), (other, 5_000)] {
        let id = coding_session::reserve_id(&mut connection)
            .await
            .expect("reserve");
        let session = coding_session::create(
            &mut connection,
            &NewCodingSession {
                id,
                project_id: Some(owner),
                title: "Session".to_owned(),
                github_credential_id: None,
                action_item_id: None,
                studio_item_id: None,
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("session")
        .id;
        let outcome = TurnOutcome {
            cost_usd_micros: Some(cost),
            input_tokens: Some(1_000),
            output_tokens: Some(200),
            ..TurnOutcome::without_result(TurnStatus::Completed, "")
        };
        session_event::record(
            &mut connection,
            session,
            &StoredEvent {
                sequence: 1,
                turn_id: Some(Uuid::now_v7()),
                occurred_at: Utc::now(),
                event: RunnerEvent::Finished(outcome),
            },
        )
        .await
        .expect("recorded");
    }

    let (_status, list) = call(
        &state,
        Method::GET,
        &format!("/api/v1/projects/{project}/model-settings"),
        None,
    )
    .await;
    assert_eq!(
        listed(&list, "kinds", "worker")["usage"],
        json!({ "turns": 2, "costUsdMicros": 150_000, "inputTokens": 2_000, "outputTokens": 400 })
    );
    let (_status, list) = call(&state, Method::GET, "/api/v1/model-settings", None).await;
    assert_eq!(
        listed(&list, "kinds", "worker")["usage"]["costUsdMicros"],
        155_000,
        "the workspace totals every project's"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_flag_a_step_without_a_model_a_workspace_kind_in_a_project_and_unknown_names_are_refused()
{
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let project = new_project(&mut connection, "Elysium").await;
    let elysia_in_project = format!("/api/v1/projects/{project}/model-settings/elysia");
    let unknown_project = format!("/api/v1/projects/{}/model-settings/worker", Uuid::now_v7());
    let opus = json!({ "model": "claude-opus-5-5", "effort": "low" });

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
            opus.clone(),
            StatusCode::CONFLICT,
        ),
        (
            "/api/v1/model-settings/correct_spelling",
            opus.clone(),
            StatusCode::NOT_FOUND,
        ),
        (
            "/api/v1/model-settings/kinds/intern",
            opus.clone(),
            StatusCode::NOT_FOUND,
        ),
        (
            elysia_in_project.as_str(),
            opus.clone(),
            StatusCode::CONFLICT,
        ),
        (
            unknown_project.as_str(),
            opus.clone(),
            StatusCode::NOT_FOUND,
        ),
    ] {
        let (status, answer) = call(&state, Method::PUT, path, Some(&body)).await;
        assert_eq!(status, expected, "{path} {body}: {answer}");
    }
    let (_status, refused) = call(&state, Method::PUT, &elysia_in_project, Some(&opus)).await;
    assert_eq!(refused["code"], "kind_not_in_projects");
    for kind in ["model_setting_changed", "kind_model_setting_changed"] {
        assert!(
            setting_events(&mut connection, kind).await.is_empty(),
            "nothing refused was recorded"
        );
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn only_admins_change_the_workspaces_settings_and_members_set_their_projects() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let project = new_project(&mut connection, "Elysium").await;
    diesel::sql_query(format!(
        "UPDATE users SET role = 'member' WHERE id = '{TEST_PERSON_ID}'"
    ))
    .execute(&mut connection)
    .await
    .expect("the role changes");

    let haiku = json!({ "model": "claude-haiku-5-5", "effort": "low" });
    for (method, path, body) in [
        (Method::GET, "/api/v1/model-settings", None),
        (Method::PUT, "/api/v1/model-settings/elysia", Some(&haiku)),
        (Method::DELETE, "/api/v1/model-settings/elysia", None),
        (
            Method::PUT,
            "/api/v1/model-settings/kinds/worker",
            Some(&haiku),
        ),
        (Method::DELETE, "/api/v1/model-settings/kinds/worker", None),
    ] {
        let (status, refused) = call(&state, method.clone(), path, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
        assert_eq!(refused["code"], "admin_only", "{method} {path}");
    }

    let (status, set) = call(
        &state,
        Method::PUT,
        &format!("/api/v1/projects/{project}/model-settings/summaries"),
        Some(&json!({ "model": "claude-sonnet-5-5", "effort": "low" })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a member edits their project's: {set}"
    );
    let resolved = agent_models::for_agent(&mut connection, Agent::TurnSummary, Some(project))
        .await
        .expect("resolves");
    assert_eq!(resolved.source, Source::Project);
}
