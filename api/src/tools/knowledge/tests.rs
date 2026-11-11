// Copyright © 2026 Jalapeno Labs

//! The `elysium_knowledge` tools: their names and arguments against their schemas, and, driven
//! through the router at `/api/agent-tools/elysium_knowledge` under a turn's grant (#96), a
//! session's agent finding, opening, and listing what the company recorded, a session a guest
//! started refused every tool, its searches spending its starter's budget, Elysia reading as
//! herself, and a turn's pages (#205): an edit a version naming who started the session and its
//! agent, a stale save, the company doc and a learning refusing it, and who may write at all; and
//! suggesting an edit (#262), to a page or a company doc section, recorded as every write is.
//!
//! The router tests are `#[ignore]`d so `cargo test` stays hermetic;
//! `api/scripts/verify-migrations.sh` runs them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use diesel_async::{AsyncPgConnection, RunQueryDsl as _};
use secrecy::ExposeSecret;
use tower::ServiceExt as _;

use super::*;
use crate::knowledge::search::PER_PERSON;
use crate::middleware::rate_limit::Decision;
use crate::models::coding_session::NewCodingSession;
use crate::models::learning::{self, LearningContent};
use crate::models::page::{self, LEARNINGS_PAGE_ID};
use crate::models::project::{self, NewProject};
use crate::routes;
use crate::routes::v1::knowledge::tests::{
    at, coworker, document_of_record, meeting_topic, record, set_role,
};
use crate::runner::{McpServer, WorkspacePath};
use crate::state::AppState;
use crate::test_support::{TEST_PERSON_ID, app_state, hold_budgets_still, migrated_database};
use crate::tools::grants::Grant;
use crate::tools::{ElysiaScope, ProjectManagerScope, SessionScope, WorkerScope};

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
            "page_read",
            "page_list",
            "page_save",
            "page_suggest",
            "page_move",
            "page_label_add",
            "page_label_remove",
            "page_project_link",
            "page_project_unlink",
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
        ("page_read", json!({ "id": id })),
        ("page_list", json!({ "parentId": id, "offset": 200 })),
        (
            "page_save",
            json!({
                "id": id,
                "baseVersion": 1,
                "parentId": id,
                "title": "Runbooks",
                "body": "Restart it.",
            }),
        ),
        (
            "page_move",
            json!({ "id": id, "to": { "parentId": null, "after": id } }),
        ),
        ("page_label_add", json!({ "id": id, "label": "runbook" })),
        ("page_label_remove", json!({ "id": id, "label": "runbook" })),
        ("page_project_link", json!({ "id": id, "projectId": id })),
        ("page_project_unlink", json!({ "id": id, "projectId": id })),
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
            "meetings_list" => parse_arguments::<ListMeetingsRequest>(&text).map(|_parsed| ()),
            "page_read" => parse_arguments::<PageArguments>(&text).map(|_parsed| ()),
            "page_list" => parse_arguments::<ListPagesArguments>(&text).map(|_parsed| ()),
            "page_save" => parse_arguments::<SavePageRequest>(&text).map(|_parsed| ()),
            "page_move" => parse_arguments::<MovePageArguments>(&text).map(|_parsed| ()),
            "page_project_link" | "page_project_unlink" => {
                parse_arguments::<PageProjectArguments>(&text).map(|_parsed| ())
            }
            _ => parse_arguments::<PageLabelArguments>(&text).map(|_parsed| ()),
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

/// The number of the session [`session_turn`] just started.
async fn newest_session(connection: &mut AsyncPgConnection) -> i64 {
    coding_session::list_coding(connection)
        .await
        .expect("the sessions are listed")
        .into_iter()
        .map(|session| session.id)
        .max()
        .expect("a session was started")
}

/// A new page at the top of the tree, saved by the turn behind `bearer`.
async fn new_page(state: &AppState, bearer: &str, title: &str, body: &str) -> Value {
    let saved = answered(
        call(
            state,
            bearer,
            "page_save",
            json!({ "parentId": null, "title": title, "body": body }),
        )
        .await,
    );
    saved["page"].clone()
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_sessions_agent_edits_a_page_as_whoever_started_it_naming_the_session() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let (_grant, bearer) = session_turn(&state, &mut connection, TEST_PERSON_ID).await;
    let session = newest_session(&mut connection).await;

    let created = new_page(&state, &bearer, "Runbooks", "First.").await;
    let id: Uuid = serde_json::from_value(created["id"].clone()).expect("an id");
    let read = answered(call(&state, &bearer, "page_read", json!({ "id": id })).await);
    assert_eq!(read["page"]["body"], "First.");

    let edited = answered(
        call(
            &state,
            &bearer,
            "page_save",
            json!({
                "id": id,
                "baseVersion": read["page"]["currentVersion"],
                "title": "Runbooks",
                "body": "Second.",
            }),
        )
        .await,
    );
    assert_eq!(edited["outcome"], "published");

    let version = page::version_summary(&mut connection, id, 2)
        .await
        .expect("every edit is a version");
    assert_eq!(version.created_by, TEST_PERSON_ID, "made by the person");
    assert_eq!(version.agent, Some(format!("session:{session}")));

    let listed = answered(call(&state, &bearer, "page_list", json!({})).await);
    assert!(
        listed["pages"]
            .as_array()
            .expect("a list")
            .iter()
            .any(|page| page["id"] == json!(id)),
        "{listed}"
    );

    let moved = answered(
        call(
            &state,
            &bearer,
            "page_move",
            json!({ "id": id, "to": { "parentId": null, "after": null } }),
        )
        .await,
    );
    assert_eq!(moved["page"]["id"], json!(id));
    let history = page::history(&mut connection, id).await.expect("read");
    let last = history.last().expect("the move is recorded");
    assert_eq!(last.kind, "moved");
    assert_eq!(last.created_by, TEST_PERSON_ID);
    assert_eq!(last.agent, Some(format!("session:{session}")));

    let labeled = answered(
        call(
            &state,
            &bearer,
            "page_label_add",
            json!({ "id": id, "label": "Runbook" }),
        )
        .await,
    );
    assert_eq!(labeled["page"]["labels"], json!(["runbook"]));
    let history = page::history(&mut connection, id).await.expect("read");
    let last = history.last().expect("the label is recorded");
    assert_eq!(last.kind, "labeled");
    assert_eq!(last.created_by, TEST_PERSON_ID);
    assert_eq!(last.agent, Some(format!("session:{session}")));

    let unlabeled = answered(
        call(
            &state,
            &bearer,
            "page_label_remove",
            json!({ "id": id, "label": "runbook" }),
        )
        .await,
    );
    assert_eq!(unlabeled["page"]["labels"], json!([]));

    let (failed, text) = call(
        &state,
        &bearer,
        "page_label_add",
        json!({ "id": id, "label": "a/b" }),
    )
    .await;
    assert!(failed && text.contains("characters"), "{text}");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_sessions_agent_links_a_page_to_a_project_naming_the_session() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let (_grant, bearer) = session_turn(&state, &mut connection, TEST_PERSON_ID).await;
    let session = newest_session(&mut connection).await;
    let created = new_page(&state, &bearer, "Runbooks", "First.").await;
    let id: Uuid = serde_json::from_value(created["id"].clone()).expect("an id");
    let project_id = project::create(
        &mut connection,
        &NewProject {
            name: "Handbook".to_owned(),
            description: String::new(),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("a project")
    .id;
    let arguments = json!({ "id": id, "projectId": project_id });

    let linked = answered(call(&state, &bearer, "page_project_link", arguments.clone()).await);
    assert_eq!(linked["page"]["projectIds"], json!([project_id]));
    assert_eq!(linked["page"]["currentVersion"], 1, "a link is no version");
    let history = page::history(&mut connection, id).await.expect("read");
    let last = history.last().expect("the link is recorded");
    assert_eq!(last.kind, "project_linked");
    assert_eq!(last.created_by, TEST_PERSON_ID);
    assert_eq!(last.agent, Some(format!("session:{session}")));

    let unlinked = answered(call(&state, &bearer, "page_project_unlink", arguments).await);
    assert_eq!(unlinked["page"]["projectIds"], json!([]));

    let unknown = json!({ "id": id, "projectId": Uuid::now_v7() });
    let (failed, text) = call(&state, &bearer, "page_project_link", unknown).await;
    assert!(failed, "{text}");
    assert!(text.contains("no project"), "{text}");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_turns_stale_save_and_wrong_arguments_say_what_to_do() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let (_grant, bearer) = session_turn(&state, &mut connection, TEST_PERSON_ID).await;
    let created = new_page(&state, &bearer, "Runbooks", "First.").await;
    let edit = |body: &str| json!({ "id": created["id"], "baseVersion": 1, "title": "Runbooks", "body": body });
    answered(call(&state, &bearer, "page_save", edit("Second.")).await);

    let (failed, text) = call(&state, &bearer, "page_save", edit("Third.")).await;
    assert!(failed, "{text}");
    assert!(text.contains("page_changed"), "{text}");
    assert!(text.contains("page_read"), "{text}");
    let id: Uuid = serde_json::from_value(created["id"].clone()).expect("an id");
    let stored = page::find(&mut connection, id).await.expect("stored");
    assert_eq!(stored.body, "Second.", "nothing was overwritten");

    let (failed, text) = call(
        &state,
        &bearer,
        "page_save",
        json!({ "title": "A", "body": "" }),
    )
    .await;
    assert!(failed && text.contains("parentId"), "{text}");

    let (failed, text) = call(
        &state,
        &bearer,
        "page_read",
        json!({ "id": Uuid::now_v7() }),
    )
    .await;
    assert!(failed && text.contains("page_list"), "{text}");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn the_company_doc_refuses_every_kind_of_agent_and_points_at_proposing() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let (_session_grant, session) = session_turn(&state, &mut connection, TEST_PERSON_ID).await;
    let (_elysia_grant, elysia) = grant(
        &state,
        &CallScope::Elysia(ElysiaScope {
            turn_id: Uuid::now_v7(),
        }),
    )
    .await;
    let (_manager_grant, manager) = grant(
        &state,
        &CallScope::ProjectManager(ProjectManagerScope {
            project_id: Uuid::now_v7(),
            turn_id: Uuid::now_v7(),
        }),
    )
    .await;
    let section = Section::Glossary.page_id();
    let before = page::find(&mut connection, section).await.expect("seeded");

    for bearer in [&session, &elysia, &manager] {
        let (failed, text) = call(
            &state,
            bearer,
            "page_save",
            json!({
                "id": section,
                "baseVersion": before.current_version,
                "title": before.title,
                "body": "- ACME: a client",
            }),
        )
        .await;
        assert!(failed, "{text}");
        assert!(text.contains("page_proposal_only"), "{text}");
        assert!(text.contains("page_suggest"), "{text}");
    }
    let after = page::find(&mut connection, section).await.expect("there");
    assert_eq!(after.current_version, before.current_version);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_turn_is_told_it_cannot_change_a_learning() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let (_grant, bearer) = session_turn(&state, &mut connection, TEST_PERSON_ID).await;
    let saved = learning::save(
        &mut connection,
        LearningContent {
            title: "Lint first".to_owned(),
            body: "Run the linter before the build.".to_owned(),
            category: "convention".to_owned(),
            evidence: "It caught WAY-123.".to_owned(),
        },
        Author {
            user_id: TEST_PERSON_ID,
            agent: None,
            at: Utc::now(),
        },
    )
    .await
    .expect("saved");

    for arguments in [
        json!({
            "id": saved.learning.id,
            "baseVersion": 1,
            "title": "Lint first",
            "body": "Run the linter after the build.",
        }),
        json!({ "parentId": LEARNINGS_PAGE_ID, "title": "A learning?", "body": "Perhaps." }),
    ] {
        let (failed, text) = call(&state, &bearer, "page_save", arguments).await;
        assert!(failed, "{text}");
        assert!(text.contains(PAGE_IS_A_LEARNING), "{text}");
        assert!(
            !text.contains("learning_save"),
            "a turn has no learning_save to go to: {text}"
        );
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_session_a_guest_or_someone_disabled_since_started_is_refused_every_page_tool() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let (_grant, bearer) = session_turn(&state, &mut connection, TEST_PERSON_ID).await;
    let created = new_page(&state, &bearer, "Runbooks", "First.").await;
    let tools = [
        ("page_read", json!({ "id": created["id"] })),
        ("page_list", json!({})),
        (
            "page_save",
            json!({ "id": created["id"], "baseVersion": 1, "title": "Runbooks", "body": "Second." }),
        ),
        (
            "page_move",
            json!({ "id": created["id"], "to": { "parentId": null, "after": null } }),
        ),
        (
            "page_label_add",
            json!({ "id": created["id"], "label": "runbook" }),
        ),
        (
            "page_label_remove",
            json!({ "id": created["id"], "label": "runbook" }),
        ),
        (
            "page_project_link",
            json!({ "id": created["id"], "projectId": created["id"] }),
        ),
        (
            "page_project_unlink",
            json!({ "id": created["id"], "projectId": created["id"] }),
        ),
    ];

    // After the grant was minted: every call checks the starter again.
    set_role(&mut connection, "guest").await;
    for (tool, arguments) in tools.clone() {
        let (failed, text) = call(&state, &bearer, tool, arguments).await;
        assert!(failed && text.contains("members alone"), "{tool}: {text}");
    }

    set_role(&mut connection, "member").await;
    diesel::sql_query(format!(
        "UPDATE users SET status = 'disabled' WHERE id = '{TEST_PERSON_ID}'"
    ))
    .execute(&mut connection)
    .await
    .expect("disabled");
    for (tool, arguments) in tools {
        let (failed, text) = call(&state, &bearer, tool, arguments).await;
        assert!(failed && text.contains("members alone"), "{tool}: {text}");
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn elysia_and_project_managers_write_as_themselves() {
    let (url, _connection) = migrated_database().await;
    let state = app_state(&url).await;
    let (_elysia_grant, elysia) = grant(
        &state,
        &CallScope::Elysia(ElysiaScope {
            turn_id: Uuid::now_v7(),
        }),
    )
    .await;
    let project_id = Uuid::now_v7();
    let (_manager_grant, manager) = grant(
        &state,
        &CallScope::ProjectManager(ProjectManagerScope {
            project_id,
            turn_id: Uuid::now_v7(),
        }),
    )
    .await;

    let hers = new_page(&state, &elysia, "Elysia's notes", "Hers.").await;
    assert_eq!(hers["publishedBy"], json!(ELYSIA_USER_ID));
    assert_eq!(hers["agent"], Value::Null, "she is not acting for anyone");

    let its = new_page(&state, &manager, "Launch plan", "Its.").await;
    assert_eq!(its["publishedBy"], json!(PROJECT_MANAGER_USER_ID));
    assert_eq!(its["agent"]["id"], format!("project_manager:{project_id}"));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_worker_reads_pages_and_is_sent_to_its_project_manager_to_change_one() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let (_session_grant, session) = session_turn(&state, &mut connection, TEST_PERSON_ID).await;
    let created = new_page(&state, &session, "Runbooks", "First.").await;
    let (_grant, worker) = grant(
        &state,
        &CallScope::Worker(WorkerScope {
            batch_id: Uuid::now_v7(),
            project_id: Uuid::now_v7(),
        }),
    )
    .await;

    let read = answered(call(&state, &worker, "page_read", json!({ "id": created["id"] })).await);
    assert_eq!(read["page"]["body"], "First.");

    for (tool, arguments) in [
        (
            "page_save",
            json!({ "id": created["id"], "baseVersion": 1, "title": "Runbooks", "body": "Second." }),
        ),
        (
            "page_move",
            json!({ "id": created["id"], "to": { "parentId": null, "after": null } }),
        ),
        (
            "page_label_add",
            json!({ "id": created["id"], "label": "runbook" }),
        ),
        (
            "page_label_remove",
            json!({ "id": created["id"], "label": "runbook" }),
        ),
        (
            "page_project_link",
            json!({ "id": created["id"], "projectId": created["id"] }),
        ),
        (
            "page_project_unlink",
            json!({ "id": created["id"], "projectId": created["id"] }),
        ),
        (
            "page_suggest",
            json!({
                "id": created["id"],
                "baseVersion": 1,
                "edits": [{ "find": "First.", "replace": "Second." }],
                "reason": "It changed.",
            }),
        ),
    ] {
        let (failed, text) = call(&state, &worker, tool, arguments).await;
        assert!(failed, "{tool}: {text}");
        assert!(text.contains("worker_finish"), "{tool}: {text}");
    }
    let id: Uuid = serde_json::from_value(created["id"].clone()).expect("an id");
    let stored = page::find(&mut connection, id).await.expect("stored");
    assert_eq!(stored.current_version, 1);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_sessions_agent_suggests_an_edit_as_whoever_started_it_and_the_company_doc_takes_one() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let (_grant, bearer) = session_turn(&state, &mut connection, TEST_PERSON_ID).await;
    let session = newest_session(&mut connection).await;
    let created = new_page(&state, &bearer, "Runbooks", "Restart the worker.").await;
    let id: Uuid = serde_json::from_value(created["id"].clone()).expect("an id");

    let suggested = answered(
        call(
            &state,
            &bearer,
            "page_suggest",
            json!({
                "id": id,
                "baseVersion": 1,
                "edits": [{ "find": "the worker.", "replace": "the worker, then the queue." }],
                "reason": "The queue holds jobs the worker drops.",
                "quote": "restart the queue too",
            }),
        )
        .await,
    );
    assert_eq!(suggested["created"], true);
    assert_eq!(suggested["suggestion"]["createdBy"], json!(TEST_PERSON_ID));
    assert_eq!(
        suggested["suggestion"]["agent"]["id"],
        json!(format!("session:{session}"))
    );
    let stored = page::find(&mut connection, id).await.expect("stored");
    assert_eq!(
        stored.body, "Restart the worker.",
        "nothing changes until accepted"
    );

    let read = answered(call(&state, &bearer, "page_read", json!({ "id": id })).await);
    assert_eq!(read["page"]["pendingSuggestions"], 1);
    assert_eq!(
        read["suggestions"][0]["changes"][0]["after"], "Restart the worker, then the queue.",
        "a page's reader sees what waits"
    );

    let proposed = answered(
        call(
            &state,
            &bearer,
            "company_doc_propose",
            json!({
                "section": "glossary",
                "edits": [{ "find": "", "replace": "- WAY: the tracker" }],
                "reason": "Named in the launch thread.",
            }),
        )
        .await,
    );
    assert_eq!(proposed["pageId"], json!(Section::Glossary.page_id()));
    let doc = answered(call(&state, &bearer, "company_doc_read", json!({})).await);
    let glossary = doc["sections"]
        .as_array()
        .expect("sections")
        .iter()
        .find(|section| section["section"] == "glossary")
        .expect("a glossary");
    assert_eq!(glossary["proposals"][0]["id"], proposed["proposal"]["id"]);
    assert_eq!(
        glossary["proposals"][0]["agent"]["id"],
        json!(format!("session:{session}"))
    );
}
