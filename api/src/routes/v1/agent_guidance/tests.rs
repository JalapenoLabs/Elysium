// Copyright © 2026 Jalapeno Labs

//! The agent guidance against Postgres (#98): admins saving and restoring versions, every one
//! kept with who wrote it, a save made against an older version refused, members reading it with
//! its `CLAUDE.md` block, guests reaching none of it, and every new session, over MCP and in a
//! turn, reading the version current as it connects.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use tower::ServiceExt as _;

use crate::oauth::SCOPE_READ;
use crate::routes;
use crate::runner::{McpServer, WorkspacePath};
use crate::state::AppState;
use crate::test_support::{
    TEST_PERSON_ID, app_state, mcp_bearer, migrated_database, signed_in_cookie,
};
use crate::tools::{CallScope, SessionScope, knowledge};

async fn call(
    state: &AppState,
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
    answer(state, request).await
}

async fn answer(state: &AppState, request: Request<Body>) -> (StatusCode, Value) {
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

async fn set_role(connection: &mut AsyncPgConnection, role: &str) {
    diesel::sql_query(format!(
        "UPDATE users SET role = '{role}' WHERE id = '{TEST_PERSON_ID}'"
    ))
    .execute(connection)
    .await
    .expect("the role changes");
}

async fn save(state: &AppState, content: &str, base_version: i64) -> (StatusCode, Value) {
    call(
        state,
        Method::PUT,
        "/api/v1/agent-guidance",
        Some(&json!({ "content": content, "baseVersion": base_version })),
    )
    .await
}

async fn restore(state: &AppState, version: i64) -> (StatusCode, Value) {
    let path = format!("/api/v1/agent-guidance/versions/{version}/restore");
    call(state, Method::POST, &path, None).await
}

/// The instructions a client is told as it connects to `path` with `authorization`, as Claude
/// Code's `initialize` asks for them.
async fn instructions_at(state: &AppState, path: &str, host: &str, authorization: &str) -> String {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "1" },
        },
    });
    let request = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::HOST, host)
        .header(header::AUTHORIZATION, authorization)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .body(Body::from(body.to_string()))
        .expect("a request builds");
    let (status, answered) = answer(state, request).await;
    assert_eq!(status, StatusCode::OK, "{path}: {answered}");
    answered["result"]["instructions"]
        .as_str()
        .unwrap_or_else(|| panic!("{path} answers its instructions: {answered}"))
        .to_owned()
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn admins_save_and_restore_versions_and_every_one_is_kept() {
    let (url, _connection) = migrated_database().await;
    let state = app_state(&url).await;

    let (status, first) = call(&state, Method::GET, "/api/v1/agent-guidance", None).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["current"]["version"], 2);
    assert_eq!(first["versions"].as_array().map(Vec::len), Some(2));
    assert!(
        first["rules"]
            .as_str()
            .is_some_and(|rules| rules.contains("http://localhost:4000/knowledge/documents/")),
        "{first}"
    );

    let (status, saved) = save(&state, "  Search the knowledge base first.  ", 2).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["current"]["version"], 3);
    assert_eq!(
        saved["current"]["content"], "Search the knowledge base first.",
        "the text is trimmed"
    );
    assert_eq!(saved["current"]["createdBy"], TEST_PERSON_ID.to_string());
    assert!(
        saved["claudeMd"]
            .as_str()
            .is_some_and(|block| block.contains("version 3")
                && block.contains("Search the knowledge base first.")),
        "{saved}"
    );

    let (status, unchanged) = save(&state, "Search the knowledge base first.", 3).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        unchanged["current"]["version"], 3,
        "saving the same text records nothing"
    );

    let (status, refused) = save(&state, "An edit of version 2.", 2).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "guidance_changed");

    let max = saved["maxCharacters"]
        .as_u64()
        .expect("the limit is a number");
    let too_long = "x".repeat(usize::try_from(max).expect("it fits") + 1);
    let (status, _) = save(&state, &too_long, 3).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, restored) = restore(&state, 2).await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["current"]["version"], 4);
    assert_eq!(restored["current"]["restoredFrom"], 2);
    assert_eq!(restored["current"]["content"], first["current"]["content"]);
    let numbers: Vec<i64> = restored["versions"]
        .as_array()
        .expect("listed")
        .iter()
        .filter_map(|version| version["version"].as_i64())
        .collect();
    assert_eq!(numbers, [4, 3, 2, 1], "every version is kept, newest first");

    let (status, again) = restore(&state, 2).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        again["current"]["version"], 4,
        "restoring the text already current records nothing"
    );
    let (status, _) = restore(&state, 99).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn members_read_the_guidance_admins_alone_change_it_and_guests_reach_none_of_it() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let (status, saved) = save(&state, "Search the knowledge base first.", 2).await;
    assert_eq!(status, StatusCode::OK, "{saved}");

    set_role(&mut connection, "member").await;
    let (status, read) = call(&state, Method::GET, "/api/v1/agent-guidance", None).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a member reads it, to copy its block"
    );
    assert_eq!(read["current"]["version"], 3);
    let (status, refused) = save(&state, "A member's edit.", 3).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(refused["code"], "admin_only");
    let (status, refused) = restore(&state, 1).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(refused["code"], "admin_only");

    set_role(&mut connection, "guest").await;
    let (status, refused) = call(&state, Method::GET, "/api/v1/agent-guidance", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(refused["code"], "members_only");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn every_new_session_reads_the_version_current_as_it_connects() {
    let (url, _connection) = migrated_database().await;
    let state = app_state(&url).await;
    let bearer = mcp_bearer(&state, &[SCOPE_READ]).await;
    let grant = state
        .grants
        .grant(
            &CallScope::Session(SessionScope {
                session_id: 1,
                project_id: None,
                action_item_id: None,
                directory: WorkspacePath::new("sessions/1").expect("a path"),
            }),
            &[&knowledge::SERVER],
        )
        .await
        .expect("a grant is stored");
    let McpServer::Http { headers, .. } = &grant.mcp_servers()[0] else {
        panic!("tools are served over HTTP");
    };
    let turn = headers[0].1.expose_secret().to_owned();

    let client = instructions_at(&state, "/api/mcp", "localhost", &bearer).await;
    assert!(
        client.starts_with("Elysium is a shared workspace"),
        "{client}"
    );
    assert!(
        client.contains("search the knowledge base with knowledge_search"),
        "{client}"
    );
    assert!(
        client.ends_with("http://localhost:4000/knowledge/documents/<documentId>."),
        "{client}"
    );

    let (status, saved) = save(&state, "Ask in #platform before changing CI.", 2).await;
    assert_eq!(status, StatusCode::OK, "{saved}");

    let client = instructions_at(&state, "/api/mcp", "localhost", &bearer).await;
    assert!(
        client.contains("Ask in #platform before changing CI."),
        "{client}"
    );
    assert!(
        !client.contains("search the knowledge base with knowledge_search"),
        "the version it replaced is gone: {client}"
    );
    assert!(
        client.contains("never for repeating outside the company"),
        "{client}"
    );

    let in_turn = instructions_at(
        &state,
        "/api/agent-tools/elysium_knowledge",
        "api:8080",
        &turn,
    )
    .await;
    assert!(
        in_turn.starts_with("The company's knowledge base"),
        "{in_turn}"
    );
    assert!(
        in_turn.contains("Ask in #platform before changing CI."),
        "{in_turn}"
    );
    assert!(
        in_turn.contains("never for repeating outside the company"),
        "{in_turn}"
    );

    grant.revoke().await;
}
