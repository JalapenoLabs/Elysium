// Copyright © 2026 Jalapeno Labs

//! The knowledge tools: their names and schemas, and, driven through the router at `/api/mcp`
//! (#96), a member's client finding, opening, and listing what a coworker recorded, a guest's
//! client refused every tool, a client's searches spending the person's one budget, and a
//! client asking for a written answer (#97).
//!
//! The router tests are `#[ignore]`d so `cargo test` stays hermetic;
//! `api/scripts/verify-migrations.sh` runs them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use serde_json::Value;
use tower::ServiceExt as _;
use uuid::Uuid;

use super::*;
use crate::knowledge::search::PER_PERSON;
use crate::middleware::rate_limit::Decision;
use crate::oauth::SCOPE_READ;
use crate::routes;
use crate::routes::v1::knowledge::tests::{
    at, coworker, document_of_record, get, meeting_topic, record, set_role,
};
use crate::state::AppState;
use crate::test_support::{
    TEST_PERSON_ID, app_state, hold_budgets_still, mcp_bearer, migrated_database,
};

#[test]
fn the_tools_are_listed_under_the_names_the_knowledge_base_gives_them() {
    let names: Vec<String> = Workspace::knowledge_router()
        .list_all()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();
    assert_eq!(names.len(), 5);
    for name in [
        "knowledge_search",
        "knowledge_get",
        "meetings_list",
        "knowledge_ask",
        "knowledge_ask_get",
    ] {
        assert!(names.contains(&name.to_owned()), "{name}: {names:?}");
    }
}

#[test]
fn every_argument_is_described_and_no_other_is_taken() {
    let expected: [(&str, &[&str]); 5] = [
        (
            "knowledge_search",
            &[
                "query", "sources", "project", "person", "from", "to", "limit",
            ],
        ),
        (
            "knowledge_get",
            &["documentId", "fromPosition", "toPosition"],
        ),
        (
            "meetings_list",
            &["from", "to", "person", "project", "limit"],
        ),
        (
            "knowledge_ask",
            &["question", "sources", "project", "person", "from", "to"],
        ),
        ("knowledge_ask_get", &["id"]),
    ];
    let tools = Workspace::knowledge_router().list_all();
    for (name, arguments) in expected {
        let tool = tools.iter().find(|tool| tool.name == name).expect("listed");
        let properties = tool.input_schema["properties"]
            .as_object()
            .expect("the arguments are an object");
        assert_eq!(properties.len(), arguments.len(), "{name}: {properties:?}");
        for argument in arguments {
            let described = properties[*argument]["description"]
                .as_str()
                .is_some_and(|description| !description.is_empty());
            assert!(described, "{name}.{argument} is described");
        }
        assert_eq!(tool.input_schema["additionalProperties"], false, "{name}");
    }
}

/// What the MCP server answered to calling `tool` with `arguments` over `bearer`: the tool's
/// result, which is an error result when the tool refused.
async fn call(state: &AppState, bearer: &str, tool: &str, arguments: Value) -> Value {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": tool, "arguments": arguments },
    });
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/mcp")
        .header(header::HOST, "localhost")
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
    answer["result"].clone()
}

/// The structured content of a result that succeeded.
fn content(result: &Value) -> &Value {
    assert_eq!(result["isError"], false, "{result}");
    &result["structuredContent"]
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_members_client_finds_opens_and_lists_what_a_coworker_recorded() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let bearer = mcp_bearer(&state, &[SCOPE_READ]).await;
    let jamie = coworker(&mut connection, "Jamie").await;
    let record_id = record(
        &mut connection,
        jamie,
        "slack",
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

    let found = call(
        &state,
        &bearer,
        "knowledge_search",
        json!({ "query": "launch Thursday", "sources": ["slack"] }),
    )
    .await;
    let found = content(&found);
    assert_eq!(
        found["results"].as_array().map(Vec::len),
        Some(1),
        "{found}"
    );
    let citation = &found["results"][0]["citation"];
    assert_eq!(citation["title"], "Launch plan");
    assert_eq!(
        citation["person"]["name"], "Jamie",
        "a coworker's message is found"
    );
    assert_eq!(citation["documentId"], json!(document_id));

    let opened = call(
        &state,
        &bearer,
        "knowledge_get",
        json!({ "documentId": document_id }),
    )
    .await;
    let document = &content(&opened)["document"];
    assert_eq!(document["citation"]["title"], "Launch plan");
    assert!(
        document["passages"][0]["content"]
            .as_str()
            .is_some_and(|text| text.contains("so QA can finish")),
        "{document}"
    );

    let listed = call(&state, &bearer, "meetings_list", json!({})).await;
    let meeting = &content(&listed)["meetings"][0];
    assert_eq!(meeting["title"], "Weekly sync");
    assert_eq!(meeting["topics"][0]["title"], "Launch date");

    let missing = call(
        &state,
        &bearer,
        "knowledge_get",
        json!({ "documentId": Uuid::now_v7() }),
    )
    .await;
    assert_eq!(missing["isError"], true);
    assert_eq!(missing["structuredContent"]["status"], 404);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_guests_client_is_refused_every_knowledge_tool() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let bearer = mcp_bearer(&state, &[SCOPE_READ]).await;
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
    set_role(&mut connection, "guest").await;

    for (tool, arguments) in [
        ("knowledge_search", json!({ "query": "launch" })),
        ("knowledge_get", json!({ "documentId": document_id })),
        ("meetings_list", json!({})),
        (
            "knowledge_ask",
            json!({ "question": "When is the launch?" }),
        ),
        ("knowledge_ask_get", json!({ "id": Uuid::now_v7() })),
    ] {
        let refused = call(&state, &bearer, tool, arguments).await;
        assert_eq!(refused["isError"], true, "{tool}: {refused}");
        let body = &refused["structuredContent"];
        assert_eq!(body["status"], 403, "{tool}: {body}");
        assert_eq!(body["code"], "members_only", "{tool}: {body}");
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_clients_searches_spend_the_persons_one_budget() {
    let (url, _connection) = migrated_database().await;
    let mut state = app_state(&url).await;
    // Time held still, so the bucket cannot refill while the tool and the route spend it.
    hold_budgets_still(&mut state);
    let bearer = mcp_bearer(&state, &[SCOPE_READ]).await;

    // All but one spent here, so the tool spends the last.
    for _ in 1..PER_PERSON.burst {
        assert!(matches!(
            state.knowledge.admit(TEST_PERSON_ID),
            Decision::Admitted { .. }
        ));
    }
    let last = call(&state, &bearer, "knowledge_search", json!({ "query": "x" })).await;
    content(&last);

    let refused = call(&state, &bearer, "knowledge_search", json!({ "query": "x" })).await;
    assert_eq!(refused["isError"], true, "{refused}");
    assert_eq!(refused["structuredContent"]["status"], 429);
    assert_eq!(
        refused["structuredContent"]["code"],
        "knowledge_search_rate_limited"
    );
    let (status, _) = get(&state, "/api/v1/knowledge/search?q=x").await;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "the browser spends the same budget"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_client_asks_and_reads_its_answer_back_as_the_person() {
    let (url, _connection) = migrated_database().await;
    let state = app_state(&url).await;
    let bearer = mcp_bearer(&state, &[SCOPE_READ]).await;
    // Nothing the search finds answers it, so it is answered within the call, without a turn.
    let asked = call(
        &state,
        &bearer,
        "knowledge_ask",
        json!({ "question": "What is ZZZ-999?" }),
    )
    .await;
    let asked = &content(&asked)["ask"];
    assert_eq!(asked["state"], "answered", "{asked}");
    assert_eq!(asked["answer"]["claims"], json!([]));

    let read = call(
        &state,
        &bearer,
        "knowledge_ask_get",
        json!({ "id": asked["id"] }),
    )
    .await;
    assert_eq!(content(&read)["ask"], *asked, "the same ask, read back");

    let missing = call(
        &state,
        &bearer,
        "knowledge_ask_get",
        json!({ "id": Uuid::now_v7() }),
    )
    .await;
    assert_eq!(missing["isError"], true);
    assert_eq!(missing["structuredContent"]["status"], 404);
}
