// Copyright © 2026 Jalapeno Labs

//! The page tools: their schemas, and, driven through the router at `/api/mcp` (#205, #206), a
//! member's client reading, editing, labelling, and linking a page, its edit a version naming the
//! person and the client, a stale save, the company doc and a learning refusing it, a read-only
//! grant, a guest's client, and a client that could not be named.
//!
//! The router tests are `#[ignore]`d so `cargo test` stays hermetic;
//! `api/scripts/verify-migrations.sh` runs them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use serde_json::Value;
use tower::ServiceExt as _;
use uuid::Uuid;

use super::*;
use crate::models::company_doc::Section;
use crate::models::learning::{self, LearningContent};
use crate::models::page::{self, COMPANY_PAGE_ID, LEARNINGS_PAGE_ID};
use crate::models::project::{self, NewProject};
use crate::oauth::{SCOPE_READ, SCOPE_WRITE};
use crate::routes;
use crate::routes::v1::knowledge::tests::set_role;
use crate::test_support::{
    TEST_PERSON_ID, app_state, mcp_bearer, mcp_bearer_of_client, migrated_database,
};
use crate::tools::knowledge::SERVER;

#[test]
fn the_tools_are_listed_and_every_argument_is_described() {
    let tools = Workspace::pages_router().list_all();
    let expected: [(&str, &[&str]); 8] = [
        ("page_read", &["id"]),
        ("page_list", &["parentId", "offset"]),
        (
            "page_save",
            &["id", "baseVersion", "parentId", "title", "body"],
        ),
        ("page_move", &["id", "to"]),
        ("page_label_add", &["id", "label"]),
        ("page_label_remove", &["id", "label"]),
        ("page_project_link", &["id", "projectId"]),
        ("page_project_unlink", &["id", "projectId"]),
    ];
    assert_eq!(tools.len(), expected.len());
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

#[test]
fn a_turn_reads_the_same_tools_a_client_does() {
    let over_mcp = Workspace::pages_router().list_all();
    for tool in over_mcp {
        let in_a_turn = SERVER
            .tools
            .iter()
            .find(|served| served.name == tool.name)
            .expect("every page tool is on elysium_knowledge");
        assert_eq!(tool.description.as_deref(), Some(in_a_turn.description));
        assert_eq!(
            Value::Object((*tool.input_schema).clone()),
            (in_a_turn.input_schema)(),
            "{}: rmcp's generator makes both, so they cannot drift",
            tool.name
        );
    }
}

#[test]
fn a_move_must_say_both_where_and_after_what() {
    // The route requires both through serde alone, which a schema cannot see, and a model only
    // reads the schema: it would leave them out and be refused.
    let schema = serde_json::to_value(schemars::schema_for!(MovePageArguments))
        .expect("the schema serializes");
    let required = schema["$defs"]["MovePageRequest"]["required"]
        .as_array()
        .expect("the place lists what it requires");
    assert!(required.contains(&json!("parentId")), "{schema}");
    assert!(required.contains(&json!("after")), "{schema}");
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

/// The refusal a result carries.
fn refusal(result: &Value) -> &Value {
    assert_eq!(result["isError"], true, "{result}");
    &result["structuredContent"]
}

/// A new page at the top of the tree, saved by a client over `bearer`.
async fn new_page(state: &AppState, bearer: &str, title: &str, body: &str) -> Value {
    let saved = call(
        state,
        bearer,
        "page_save",
        json!({ "parentId": null, "title": title, "body": body }),
    )
    .await;
    content(&saved)["page"].clone()
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_members_client_writes_pages_and_each_version_names_the_person_and_the_client() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let bearer = mcp_bearer(&state, &[SCOPE_READ, SCOPE_WRITE]).await;

    let created = call(
        &state,
        &bearer,
        "page_save",
        json!({ "parentId": null, "title": "Runbooks", "body": "" }),
    )
    .await;
    assert_eq!(content(&created)["outcome"], "created");
    let parent = &content(&created)["page"];

    let child = call(
        &state,
        &bearer,
        "page_save",
        json!({ "parentId": parent["id"], "title": "Restarting", "body": "Restart it." }),
    )
    .await;
    let child = &content(&child)["page"];
    assert_eq!(child["parentId"], parent["id"]);
    assert_eq!(child["agent"]["id"], "mcp:test-client");

    let read = call(&state, &bearer, "page_read", json!({ "id": child["id"] })).await;
    let read = &content(&read)["page"];
    assert_eq!(read["body"], "Restart it.");

    let edited = call(
        &state,
        &bearer,
        "page_save",
        json!({
            "id": read["id"],
            "baseVersion": read["currentVersion"],
            "title": "Restarting",
            "body": "Drain it, then restart it.",
        }),
    )
    .await;
    assert_eq!(content(&edited)["outcome"], "published");
    assert_eq!(content(&edited)["page"]["currentVersion"], 2);

    let id: Uuid = serde_json::from_value(read["id"].clone()).expect("an id");
    let version = page::version_summary(&mut connection, id, 2)
        .await
        .expect("every edit is a version");
    assert_eq!(version.created_by, TEST_PERSON_ID, "made by the person");
    assert_eq!(
        version.agent.as_deref(),
        Some("mcp:test-client"),
        "through their client"
    );

    let same = call(
        &state,
        &bearer,
        "page_save",
        json!({
            "id": read["id"],
            "baseVersion": 1,
            "title": "Restarting",
            "body": "Drain it, then restart it.",
        }),
    )
    .await;
    assert_eq!(
        content(&same)["outcome"],
        "unchanged",
        "what the page already says makes no version, whatever it was made from"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_members_client_walks_the_tree_a_branch_at_a_time_and_moves_a_page() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let bearer = mcp_bearer(&state, &[SCOPE_READ, SCOPE_WRITE]).await;
    let parent = new_page(&state, &bearer, "Runbooks", "").await;
    let saved = call(
        &state,
        &bearer,
        "page_save",
        json!({ "parentId": parent["id"], "title": "Restarting", "body": "Restart it." }),
    )
    .await;
    let read = &content(&saved)["page"];

    let at_the_top = call(&state, &bearer, "page_list", json!({})).await;
    let at_the_top = content(&at_the_top)["pages"]
        .as_array()
        .expect("a list")
        .clone();
    let listed = at_the_top
        .iter()
        .find(|page| page["id"] == parent["id"])
        .expect("the new page is at the top of the tree");
    assert_eq!(listed["childCount"], 1, "{listed}");
    assert!(
        at_the_top
            .iter()
            .all(|page| page["parentId"] == Value::Null),
        "a branch at a time: {at_the_top:?}"
    );
    let under = call(
        &state,
        &bearer,
        "page_list",
        json!({ "parentId": parent["id"] }),
    )
    .await;
    assert_eq!(content(&under)["pages"][0]["id"], read["id"]);

    let moved = call(
        &state,
        &bearer,
        "page_move",
        json!({ "id": read["id"], "to": { "parentId": null, "after": parent["id"] } }),
    )
    .await;
    assert_eq!(content(&moved)["page"]["parentId"], Value::Null);
    let id: Uuid = serde_json::from_value(read["id"].clone()).expect("an id");
    let history = page::history(&mut connection, id).await.expect("read");
    let last = history.last().expect("the move is recorded");
    assert_eq!(last.kind, "moved");
    assert_eq!(last.created_by, TEST_PERSON_ID);
    assert_eq!(last.agent.as_deref(), Some("mcp:test-client"));

    let missing = call(
        &state,
        &bearer,
        "page_list",
        json!({ "parentId": Uuid::now_v7() }),
    )
    .await;
    assert_eq!(refusal(&missing)["status"], 404);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_members_client_labels_any_page_and_makes_no_version() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let bearer = mcp_bearer(&state, &[SCOPE_READ, SCOPE_WRITE]).await;
    let created = new_page(&state, &bearer, "Runbooks", "First.").await;
    let id: Uuid = serde_json::from_value(created["id"].clone()).expect("an id");

    let labeled = call(
        &state,
        &bearer,
        "page_label_add",
        json!({ "id": id, "label": "On Call" }),
    )
    .await;
    let labeled = &content(&labeled)["page"];
    assert_eq!(labeled["labels"], json!(["on-call"]), "one spelling");
    assert_eq!(labeled["currentVersion"], 1, "a label is no version");
    let history = page::history(&mut connection, id).await.expect("read");
    let last = history.last().expect("the label is recorded");
    assert_eq!(last.kind, "labeled");
    assert_eq!(last.created_by, TEST_PERSON_ID);
    assert_eq!(last.agent.as_deref(), Some("mcp:test-client"));

    let listed = call(&state, &bearer, "page_list", json!({})).await;
    let listed = content(&listed);
    let entry = listed["pages"]
        .as_array()
        .expect("a list")
        .iter()
        .find(|page| page["id"] == json!(id))
        .expect("the page is at the top of the tree");
    assert_eq!(entry["labels"], json!(["on-call"]), "{entry}");
    assert!(listed["total"].as_u64().is_some(), "{listed}");

    // What an agent only proposes to keeps its text from it, not its labels.
    let section = call(
        &state,
        &bearer,
        "page_label_add",
        json!({ "id": Section::Glossary.page_id(), "label": "reference" }),
    )
    .await;
    assert_eq!(content(&section)["page"]["labels"], json!(["reference"]));

    let removed = call(
        &state,
        &bearer,
        "page_label_remove",
        json!({ "id": id, "label": "on call" }),
    )
    .await;
    assert_eq!(content(&removed)["page"]["labels"], json!([]));
    let history = page::history(&mut connection, id).await.expect("read");
    let last = history.last().expect("taking it off is recorded");
    assert_eq!(last.kind, "unlabeled");
    assert_eq!(last.agent.as_deref(), Some("mcp:test-client"));

    let malformed = call(
        &state,
        &bearer,
        "page_label_add",
        json!({ "id": id, "label": "a/b" }),
    )
    .await;
    assert_eq!(refusal(&malformed)["status"], 422, "{malformed}");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_members_client_links_a_page_to_a_project_and_makes_no_version() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let bearer = mcp_bearer(&state, &[SCOPE_READ, SCOPE_WRITE]).await;
    let created = new_page(&state, &bearer, "Runbooks", "First.").await;
    let id: Uuid = serde_json::from_value(created["id"].clone()).expect("an id");
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
    let arguments = json!({ "id": id, "projectId": project_id });

    let linked = call(&state, &bearer, "page_project_link", arguments.clone()).await;
    let linked = &content(&linked)["page"];
    assert_eq!(linked["projectIds"], json!([project_id]));
    assert_eq!(linked["currentVersion"], 1, "a link is no version");
    let history = page::history(&mut connection, id).await.expect("read");
    let last = history.last().expect("the link is recorded");
    assert_eq!(last.kind, "project_linked");
    assert_eq!(last.created_by, TEST_PERSON_ID);
    assert_eq!(last.agent.as_deref(), Some("mcp:test-client"));

    let read = call(&state, &bearer, "page_read", json!({ "id": id })).await;
    assert_eq!(content(&read)["page"]["projectIds"], json!([project_id]));

    let unlinked = call(&state, &bearer, "page_project_unlink", arguments).await;
    assert_eq!(content(&unlinked)["page"]["projectIds"], json!([]));
    let history = page::history(&mut connection, id).await.expect("read");
    assert_eq!(
        history.last().map(|entry| entry.kind.as_str()),
        Some("project_unlinked")
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_stale_save_is_refused_and_says_how_to_try_again() {
    let (url, _connection) = migrated_database().await;
    let state = app_state(&url).await;
    let bearer = mcp_bearer(&state, &[SCOPE_READ, SCOPE_WRITE]).await;
    let page = new_page(&state, &bearer, "Runbooks", "First.").await;
    let edit = |body: &str, base_version: i64| {
        json!({
            "id": page["id"],
            "baseVersion": base_version,
            "title": "Runbooks",
            "body": body,
        })
    };
    content(&call(&state, &bearer, "page_save", edit("Second.", 1)).await);

    let outdated = call(&state, &bearer, "page_save", edit("Third.", 1)).await;

    let refused = refusal(&outdated);
    assert_eq!(refused["status"], 409);
    assert_eq!(refused["code"], "page_changed");
    let message = refused["message"].as_str().expect("a message");
    assert!(
        message.contains("page_read") && message.contains("currentVersion"),
        "{message}"
    );
    let read = call(&state, &bearer, "page_read", json!({ "id": page["id"] })).await;
    let read = &content(&read)["page"];
    assert_eq!(read["body"], "Second.", "nothing was overwritten");

    let retried = call(&state, &bearer, "page_save", edit("Second, then third.", 2)).await;
    assert_eq!(content(&retried)["page"]["currentVersion"], 3);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn the_company_doc_refuses_a_client_and_points_it_at_proposing() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let bearer = mcp_bearer(&state, &[SCOPE_READ, SCOPE_WRITE]).await;

    for id in [Section::Glossary.page_id(), COMPANY_PAGE_ID] {
        let before = page::find(&mut connection, id).await.expect("seeded");
        let refused = call(
            &state,
            &bearer,
            "page_save",
            json!({
                "id": id,
                "baseVersion": before.current_version,
                "title": before.title,
                "body": "- ACME: a client",
            }),
        )
        .await;
        let refused = refusal(&refused);
        assert_eq!(refused["status"], 409);
        assert_eq!(refused["code"], "page_proposal_only");
        assert!(
            refused["message"]
                .as_str()
                .is_some_and(|message| message.contains("company_doc_propose")),
            "{refused}"
        );

        let after = page::find(&mut connection, id).await.expect("still there");
        assert_eq!(after.current_version, before.current_version);
    }

    let moved = call(
        &state,
        &bearer,
        "page_move",
        json!({ "id": Section::Glossary.page_id(), "to": { "parentId": null, "after": null } }),
    )
    .await;
    assert_eq!(refusal(&moved)["code"], "page_proposal_only");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_learning_is_left_to_learning_save() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let bearer = mcp_bearer(&state, &[SCOPE_READ, SCOPE_WRITE]).await;
    let saved = learning::save(
        &mut connection,
        LearningContent {
            title: "Lint first".to_owned(),
            body: "Run the linter before the build.".to_owned(),
            category: "convention".to_owned(),
            evidence: "It caught WAY-123.".to_owned(),
        },
        crate::models::agent::Author {
            user_id: TEST_PERSON_ID,
            agent: None,
            at: Utc::now(),
        },
    )
    .await
    .expect("saved");

    let revised = call(
        &state,
        &bearer,
        "page_save",
        json!({
            "id": saved.learning.id,
            "baseVersion": 1,
            "title": "Lint first",
            "body": "Run the linter after the build.",
        }),
    )
    .await;
    let refused = refusal(&revised);
    assert_eq!(refused["code"], "page_is_a_learning");
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|message| message.contains("learning_save")),
        "{refused}"
    );

    let filed = call(
        &state,
        &bearer,
        "page_save",
        json!({ "parentId": LEARNINGS_PAGE_ID, "title": "A learning?", "body": "Perhaps." }),
    )
    .await;
    assert_eq!(
        refusal(&filed)["code"],
        "page_is_a_learning",
        "a page under Learnings would pass for one and never be listed"
    );

    let read = call(
        &state,
        &bearer,
        "page_read",
        json!({ "id": saved.learning.id }),
    )
    .await;
    assert_eq!(
        content(&read)["page"]["evidence"],
        "It caught WAY-123.",
        "a learning still reads as a page"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn arguments_that_are_neither_a_new_page_nor_a_version_are_refused_by_name() {
    let (url, _connection) = migrated_database().await;
    let state = app_state(&url).await;
    let bearer = mcp_bearer(&state, &[SCOPE_READ, SCOPE_WRITE]).await;
    let page = new_page(&state, &bearer, "Runbooks", "").await;

    for (arguments, says) in [
        (json!({ "title": "A", "body": "" }), "parentId"),
        (
            json!({ "id": page["id"], "title": "A", "body": "" }),
            "baseVersion",
        ),
        (
            json!({ "baseVersion": 1, "title": "A", "body": "" }),
            "baseVersion",
        ),
        (
            json!({
                "id": page["id"],
                "baseVersion": 1,
                "parentId": null,
                "title": "A",
                "body": "",
            }),
            "page_move",
        ),
    ] {
        let refused = call(&state, &bearer, "page_save", arguments).await;
        let refused = refusal(&refused);
        assert_eq!(refused["status"], 400, "{refused}");
        assert!(
            refused["message"]
                .as_str()
                .is_some_and(|message| message.contains(says)),
            "{says}: {refused}"
        );
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_read_only_grant_reads_and_changes_nothing() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let writer = mcp_bearer(&state, &[SCOPE_READ, SCOPE_WRITE]).await;
    let page = new_page(&state, &writer, "Runbooks", "First.").await;
    let reader = mcp_bearer(&state, &[SCOPE_READ]).await;

    let read = call(&state, &reader, "page_read", json!({ "id": page["id"] })).await;
    content(&read);

    for (tool, arguments) in [
        (
            "page_save",
            json!({ "id": page["id"], "baseVersion": 1, "title": "Runbooks", "body": "Second." }),
        ),
        (
            "page_move",
            json!({ "id": page["id"], "to": { "parentId": null, "after": null } }),
        ),
        (
            "page_label_add",
            json!({ "id": page["id"], "label": "runbook" }),
        ),
        (
            "page_label_remove",
            json!({ "id": page["id"], "label": "runbook" }),
        ),
        (
            "page_project_link",
            json!({ "id": page["id"], "projectId": page["id"] }),
        ),
        (
            "page_project_unlink",
            json!({ "id": page["id"], "projectId": page["id"] }),
        ),
    ] {
        let refused = call(&state, &reader, tool, arguments).await;
        let refused = refusal(&refused);
        assert_eq!(refused["status"], 403, "{tool}");
        assert!(
            refused["message"]
                .as_str()
                .is_some_and(|message| message.contains(SCOPE_WRITE)),
            "{tool}: {refused}"
        );
    }
    let id: Uuid = serde_json::from_value(page["id"].clone()).expect("an id");
    let stored = page::find(&mut connection, id).await.expect("stored");
    assert_eq!(stored.current_version, 1);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_guests_client_is_refused_every_page_tool() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let bearer = mcp_bearer(&state, &[SCOPE_READ, SCOPE_WRITE]).await;
    let page = new_page(&state, &bearer, "Runbooks", "First.").await;
    set_role(&mut connection, "guest").await;

    for (tool, arguments) in [
        ("page_read", json!({ "id": page["id"] })),
        ("page_list", json!({})),
        (
            "page_save",
            json!({ "id": page["id"], "baseVersion": 1, "title": "Runbooks", "body": "Second." }),
        ),
        (
            "page_move",
            json!({ "id": page["id"], "to": { "parentId": null, "after": null } }),
        ),
        (
            "page_label_add",
            json!({ "id": page["id"], "label": "runbook" }),
        ),
        (
            "page_label_remove",
            json!({ "id": page["id"], "label": "runbook" }),
        ),
        (
            "page_project_link",
            json!({ "id": page["id"], "projectId": page["id"] }),
        ),
        (
            "page_project_unlink",
            json!({ "id": page["id"], "projectId": page["id"] }),
        ),
    ] {
        let refused = call(&state, &bearer, tool, arguments).await;
        let refused = refusal(&refused);
        assert_eq!(refused["status"], 403, "{tool}: {refused}");
        assert_eq!(refused["code"], "members_only", "{tool}: {refused}");
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_client_that_cannot_be_named_never_writes_as_the_person_alone() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let bearer =
        mcp_bearer_of_client(&state, &[SCOPE_READ, SCOPE_WRITE], "an id with spaces").await;
    let section = Section::Glossary.page_id();
    let before = page::find(&mut connection, section).await.expect("seeded");

    // Named by nobody, it would read as the person, who may write what agents only propose to.
    let refused = call(
        &state,
        &bearer,
        "page_save",
        json!({
            "id": section,
            "baseVersion": before.current_version,
            "title": before.title,
            "body": "- ACME: a client",
        }),
    )
    .await;

    assert_eq!(refusal(&refused)["status"], 403, "{refused}");
    let after = page::find(&mut connection, section).await.expect("there");
    assert_eq!(after.current_version, before.current_version);
}
