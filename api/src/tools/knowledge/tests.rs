// Copyright © 2026 Jalapeno Labs

//! The `elysium_knowledge` tools: their names and arguments against their schemas, and, driven
//! through the router at `/api/agent-tools/elysium_knowledge` under a turn's grant (#96), a
//! session's agent finding, opening, and listing what the company recorded, a session a guest
//! started refused every tool, its searches spending its starter's budget, and Elysia reading
//! as herself.
//!
//! The router tests are `#[ignore]`d so `cargo test` stays hermetic;
//! `api/scripts/verify-migrations.sh` runs them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use diesel_async::AsyncPgConnection;
use secrecy::ExposeSecret;
use tower::ServiceExt as _;

use super::*;
use crate::knowledge::search::PER_PERSON;
use crate::middleware::rate_limit::Decision;
use crate::models::coding_session::NewCodingSession;
use crate::models::project::{self, NewProject};
use crate::routes;
use crate::routes::v1::knowledge::tests::{
    at, coworker, document_of_record, meeting_topic, record, set_role,
};
use crate::runner::{McpServer, WorkspacePath};
use crate::state::AppState;
use crate::test_support::{TEST_PERSON_ID, app_state, hold_budgets_still, migrated_database};
use crate::tools::grants::Grant;
use crate::tools::{ElysiaScope, SessionScope};

#[test]
fn the_tools_share_the_mcp_servers_names() {
    let names: Vec<&str> = TOOLS.iter().map(|tool| tool.name).collect();
    assert_eq!(
        names,
        [
            "knowledge_search",
            "knowledge_get",
            "meetings_list",
            "company_doc_read",
            "company_doc_propose",
        ]
    );
}

#[test]
fn every_schema_property_is_an_argument_the_tool_reads() {
    let id = Uuid::nil();
    let everything = [
        (
            "knowledge_search",
            json!({
                "query": "WAY-123",
                "sources": ["slack", "meeting"],
                "project": id,
                "person": id,
                "label": "runbook",
                "from": "2026-11-01T00:00:00Z",
                "to": "2026-11-04T00:00:00Z",
                "limit": 5,
            }),
        ),
        (
            "knowledge_get",
            json!({ "documentId": id, "fromPosition": 0, "toPosition": 2 }),
        ),
        (
            "meetings_list",
            json!({
                "from": "2026-11-01T00:00:00Z",
                "to": "2026-11-04T00:00:00Z",
                "person": id,
                "project": id,
                "limit": 10,
            }),
        ),
    ];
    for (name, arguments) in everything {
        let tool = TOOLS
            .iter()
            .find(|tool| tool.name == name)
            .expect("a tool by that name");
        let schema = (tool.input_schema)();
        let properties = schema["properties"].as_object().expect("properties");
        let given = arguments.as_object().expect("object");
        assert_eq!(
            properties.len(),
            given.len(),
            "{name}: the example sets every property"
        );
        assert!(schema.get("title").is_none(), "{name}: no Rust type name");

        let text = arguments.to_string();
        let parsed = match name {
            "knowledge_search" => {
                parse_arguments::<SearchKnowledgeRequest>(&text).map(|_parsed| ())
            }
            "knowledge_get" => parse_arguments::<DocumentArguments>(&text).map(|_parsed| ()),
            _ => parse_arguments::<ListMeetingsRequest>(&text).map(|_parsed| ()),
        };
        parsed.unwrap_or_else(|error| panic!("{name}: {error}"));
    }
}

#[test]
fn the_propose_schema_names_every_section_and_reads_as_the_request() {
    let schema = propose_schema();
    assert_eq!(
        schema["properties"]["section"]["enum"]
            .as_array()
            .map(Vec::len),
        Some(Section::ALL.len())
    );

    let request: ProposeRequest = parse_arguments(
        r#"{"section":"glossary","edits":[{"find":"","replace":"- ACME"}],"reason":"Why"}"#,
    )
    .expect("what the schema describes parses");
    assert_eq!(request.section, Section::Glossary);
    assert!(
        parse_arguments::<ProposeRequest>(r#"{"section":"glossary","edits":[]}"#).is_err(),
        "a reason is required"
    );
}

/// A session of a project of its own, started by `starter`, and a live grant to the knowledge
/// tools for its turn, with the bearer its agent sends. The grant is held, since dropping it
/// revokes it.
async fn session_turn(
    state: &AppState,
    connection: &mut AsyncPgConnection,
    starter: Uuid,
) -> (Grant, String) {
    let project_id = project::create(
        connection,
        &NewProject {
            created_by: starter,
            name: "Launch".to_owned(),
            description: String::new(),
        },
    )
    .await
    .expect("the project is created")
    .id;
    let id = coding_session::reserve_id(connection)
        .await
        .expect("a number");
    let session = coding_session::create(
        connection,
        &NewCodingSession {
            id,
            project_id: Some(project_id),
            title: "Look into the launch".to_owned(),
            github_credential_id: None,
            action_item_id: None,
            studio_item_id: None,
            created_by: starter,
        },
    )
    .await
    .expect("the session is created");
    let scope = CallScope::Session(SessionScope {
        session_id: session.id,
        project_id: Some(project_id),
        action_item_id: None,
        directory: WorkspacePath::new(format!("sessions/{}", session.id)).expect("a path"),
    });
    grant(state, &scope).await
}

/// A live grant to the knowledge tools for `scope`, and the bearer its agent sends.
async fn grant(state: &AppState, scope: &CallScope) -> (Grant, String) {
    let grant = state
        .grants
        .grant(scope, &[&SERVER])
        .await
        .expect("a grant is stored");
    let McpServer::Http { headers, .. } = &grant.mcp_servers()[0] else {
        panic!("tools are served over HTTP");
    };
    let bearer = headers[0].1.expose_secret().to_owned();
    (grant, bearer)
}

/// What the agent reads from calling `tool` with `arguments`: whether it failed, and the text.
async fn call(state: &AppState, bearer: &str, tool: &str, arguments: Value) -> (bool, String) {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": tool, "arguments": arguments },
    });
    let request = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/agent-tools/{}", SERVER.name))
        .header(header::HOST, "api:8080")
        .header(header::AUTHORIZATION, bearer)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .body(Body::from(body.to_string()))
        .expect("a request builds");
    let response = routes::router(state)
        .with_state(state.clone())
        .oneshot(request)
        .await
        .expect("the router answers");
    assert_eq!(response.status(), StatusCode::OK, "{tool}");
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("the body reads");
    let answer: Value = serde_json::from_slice(&bytes).expect("the server answers JSON");
    let result = &answer["result"];
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("{tool} answers text: {answer}"))
        .to_owned();
    (result["isError"] == true, text)
}

/// The JSON a call that succeeded answered.
fn answered((failed, text): (bool, String)) -> Value {
    assert!(!failed, "{text}");
    serde_json::from_str(&text).expect("a success is JSON")
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_sessions_agent_finds_opens_and_lists_what_the_company_recorded() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let (_grant, bearer) = session_turn(&state, &mut connection, TEST_PERSON_ID).await;
    let jamie = coworker(&mut connection, "Jamie").await;
    let record_id = record(
        &mut connection,
        jamie,
        "outlook",
        at(1),
        "Launch plan",
        "The launch moves to Thursday so QA can finish.",
    )
    .await;
    let document_id = document_of_record(&mut connection, record_id).await;
    meeting_topic(
        &mut connection,
        jamie,
        Uuid::now_v7(),
        "Weekly sync",
        0,
        at(2),
        "Launch date",
    )
    .await;

    let found = answered(
        call(
            &state,
            &bearer,
            "knowledge_search",
            json!({ "query": "launch Thursday", "sources": ["outlook"] }),
        )
        .await,
    );
    let citation = &found["results"][0]["citation"];
    assert_eq!(citation["title"], "Launch plan", "{found}");
    assert_eq!(
        citation["person"]["name"], "Jamie",
        "a coworker's mail is found"
    );

    let opened = answered(
        call(
            &state,
            &bearer,
            "knowledge_get",
            json!({ "documentId": document_id, "toPosition": 0 }),
        )
        .await,
    );
    assert_eq!(opened["document"]["citation"]["title"], "Launch plan");
    assert_eq!(opened["document"]["passageCount"], 1);

    let listed = answered(call(&state, &bearer, "meetings_list", json!({})).await);
    assert_eq!(listed["meetings"][0]["title"], "Weekly sync", "{listed}");

    let (failed, text) = call(
        &state,
        &bearer,
        "knowledge_search",
        json!({ "query": "x", "limit": 0 }),
    )
    .await;
    assert!(failed && text.contains("limit"), "{text}");
    let (failed, text) = call(
        &state,
        &bearer,
        "knowledge_get",
        json!({ "documentId": Uuid::now_v7() }),
    )
    .await;
    assert!(failed && text.contains("knowledge_search"), "{text}");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_session_a_guest_started_is_refused_every_knowledge_tool() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let (_grant, bearer) = session_turn(&state, &mut connection, TEST_PERSON_ID).await;
    let record_id = record(
        &mut connection,
        TEST_PERSON_ID,
        "slack",
        at(1),
        "Plans",
        "Plans for the launch.",
    )
    .await;
    let document_id = document_of_record(&mut connection, record_id).await;
    // After the grant was minted: every call checks the starter again.
    set_role(&mut connection, "guest").await;

    for (tool, arguments) in [
        ("knowledge_search", json!({ "query": "launch" })),
        ("knowledge_get", json!({ "documentId": document_id })),
        ("meetings_list", json!({})),
        ("company_doc_read", json!({})),
        (
            "company_doc_propose",
            json!({
                "section": "glossary",
                "edits": [{ "find": "", "replace": "- ACME: a client" }],
                "reason": "Named in the launch thread.",
            }),
        ),
    ] {
        let (failed, text) = call(&state, &bearer, tool, arguments).await;
        assert!(failed, "{tool}: {text}");
        assert!(text.contains("members alone"), "{tool}: {text}");
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_sessions_searches_spend_its_starters_budget() {
    let (url, mut connection) = migrated_database().await;
    let mut state = app_state(&url).await;
    // Time held still, so the bucket cannot refill before the tool asks it.
    hold_budgets_still(&mut state);
    let (_grant, bearer) = session_turn(&state, &mut connection, TEST_PERSON_ID).await;

    for _ in 0..PER_PERSON.burst {
        assert!(matches!(
            state.knowledge.admit(TEST_PERSON_ID),
            Decision::Admitted { .. }
        ));
    }
    let (failed, text) = call(&state, &bearer, "knowledge_search", json!({ "query": "x" })).await;

    assert!(failed, "{text}");
    assert!(text.contains("Too many knowledge searches"), "{text}");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn elysias_turns_read_the_knowledge_base_as_herself() {
    let (url, mut connection) = migrated_database().await;
    let mut state = app_state(&url).await;
    // Time held still, so the person's budget stays spent while Elysia searches.
    hold_budgets_still(&mut state);
    let (_grant, bearer) = grant(
        &state,
        &CallScope::Elysia(ElysiaScope {
            turn_id: Uuid::now_v7(),
        }),
    )
    .await;
    record(
        &mut connection,
        TEST_PERSON_ID,
        "slack",
        at(1),
        "Launch plan",
        "The launch moves to Thursday.",
    )
    .await;
    // The person's budget spent does not hold Elysia back: her searches spend her own.
    for _ in 0..PER_PERSON.burst {
        state.knowledge.admit(TEST_PERSON_ID);
    }

    let found = answered(
        call(
            &state,
            &bearer,
            "knowledge_search",
            json!({ "query": "launch" }),
        )
        .await,
    );

    assert_eq!(found["results"][0]["citation"]["title"], "Launch plan");
    assert!(matches!(
        state.knowledge.admit(ELYSIA_USER_ID),
        Decision::Admitted { .. }
    ));
}
