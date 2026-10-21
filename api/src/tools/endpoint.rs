// Copyright © 2026 Jalapeno Labs

//! `/api/agent-tools/{server}`: Elysium's tool servers, for agents in the workspace.
//!
//! Each [`ToolServer`] is one MCP server over Streamable HTTP, stateless like `/api/mcp`.
//! Agents reach it at `crate::runner::AGENT_API_URL` on the network only the API and the
//! workspace share; nginx never routes here. Every request must carry the bearer token of a
//! live [`super::grants::Grant`] that names this server, checked before the server sees it,
//! and the call then runs with the grant's scope. The tool table is the same one that
//! answers every call ([`super::dispatch`]), so a listed tool always has a handler.

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

#[cfg(doc)]
use super::grants::AGENT_TOOLS_PATH;
use super::grants::{Granted, Grants};
use super::{SERVERS, ToolContext, ToolServer, dispatch};

/// The host agents address the API by, `api` on `elysium-agents`. rmcp refuses any other
/// host, which keeps a DNS rebinding page from reaching these servers through another name.
const AGENT_HOST: &str = "api";

/// Every tool server, each at `/{server}`, for mounting at [`AGENT_TOOLS_PATH`].
pub fn router(
    context: &ToolContext,
    grants: &Grants,
    shutdown: &CancellationToken,
) -> Router<crate::state::AppState> {
    let mut router = Router::new();
    for server in SERVERS {
        let config = StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_cancellation_token(shutdown.child_token())
            .with_allowed_hosts([AGENT_HOST.to_owned()]);
        let server_context = context.clone();
        let service = StreamableHttpService::new(
            move || {
                Ok(AgentToolServer {
                    context: server_context.clone(),
                    server,
                })
            },
            Arc::new(NeverSessionManager::default()),
            config,
        );
        let checked = Router::new()
            .route_service("/", service)
            .route_layer(from_fn_with_state(
                (grants.clone(), server.name),
                require_grant,
            ));
        router = router.nest(&format!("/{}", server.name), checked);
    }
    router
}

/// Refuses a request without a live grant for this server, and hands the grant to the call.
async fn require_grant(
    State((grants, server)): State<(Grants, &'static str)>,
    mut request: Request,
    next: Next,
) -> Response {
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let Some(granted) = token.and_then(|token| grants.find(token)) else {
        event!(
            name: "tools.endpoint.refused",
            Level::WARN,
            tool.server = server,
            "a tool request carried no live grant",
        );
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !granted
        .servers
        .iter()
        .any(|granted_server| granted_server.name == server)
    {
        event!(
            name: "tools.endpoint.not_granted",
            Level::WARN,
            tool.server = server,
            "a turn called a tool server its grant does not name",
        );
        return StatusCode::NOT_FOUND.into_response();
    }
    request.extensions_mut().insert(granted);
    next.run(request).await
}

/// One tool server, answering for whichever grant the request carried.
#[derive(Debug, Clone)]
struct AgentToolServer {
    context: ToolContext,
    server: &'static ToolServer,
}

impl ServerHandler for AgentToolServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                self.server.name,
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(self.server.instructions)
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        let tools = self
            .server
            .tools
            .iter()
            .map(|tool| {
                let schema = match (tool.input_schema)() {
                    Value::Object(object) => object,
                    _not_an_object => serde_json::Map::new(),
                };
                Tool::new(tool.name, tool.description, Arc::new(schema))
            })
            .collect();
        std::future::ready(Ok(ListToolsResult {
            tools,
            ..ListToolsResult::default()
        }))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let granted = context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|parts| parts.extensions.get::<Granted>())
            .cloned()
            .ok_or_else(|| {
                ErrorData::internal_error("a tool server was reached without a checked grant", None)
            })?;
        let arguments = request
            .arguments
            .map(|arguments| Value::Object(arguments).to_string())
            .unwrap_or_default();
        let output = dispatch(
            &self.context,
            &granted.scope,
            self.server.name,
            &request.name,
            &arguments,
        )
        .await;
        let blocks = vec![ContentBlock::text(output.text)];
        let result = if output.is_error {
            CallToolResult::error(blocks)
        } else {
            CallToolResult::success(blocks)
        };
        Ok(result.into())
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Method, Request};

    use super::*;
    use secrecy::ExposeSecret;
    use serde_json::json;
    use tower::ServiceExt as _;

    use crate::routes;
    use crate::runner::{McpServer, WorkspacePath};
    use crate::state::AppState;
    use crate::test_support::{app_state, migrated_database, signed_in_cookie};
    use crate::tools::{CallScope, SessionScope, work};

    /// An MCP `initialize` to `server`, as Claude Code sends it, with `bearer` when given.
    async fn initialize(state: &AppState, server: &str, bearer: Option<&str>) -> StatusCode {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/agent-tools/{server}"))
            .header(header::HOST, "api:8080")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream");
        if let Some(bearer) = bearer {
            request = request.header(header::AUTHORIZATION, format!("Bearer {bearer}"));
        }
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
        routes::router(state)
            .with_state(state.clone())
            .oneshot(
                request
                    .body(Body::from(body.to_string()))
                    .expect("a request builds"),
            )
            .await
            .expect("the router answers")
            .status()
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
    async fn a_tool_server_answers_only_a_live_grant_that_names_it() {
        let (url, _connection) = migrated_database().await;
        let state = app_state(&url).await;

        assert_eq!(
            initialize(&state, "elysium_work", None).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            initialize(&state, "elysium_work", Some("not-a-grant")).await,
            StatusCode::UNAUTHORIZED
        );

        let grant = state.sessions.grants().grant(
            CallScope::Session(SessionScope {
                session_id: 1,
                project_id: None,
                action_item_id: None,
                directory: WorkspacePath::new("sessions/1").expect("a path"),
            }),
            &[&work::SERVER],
        );
        let McpServer::Http { headers, .. } = &grant.mcp_servers()[0] else {
            panic!("tools are served over HTTP");
        };
        let token = headers[0]
            .1
            .expose_secret()
            .strip_prefix("Bearer ")
            .expect("a bearer")
            .to_owned();

        assert_eq!(
            initialize(&state, "elysium_work", Some(&token)).await,
            StatusCode::OK
        );
        assert_eq!(
            initialize(&state, "elysium_storage", Some(&token)).await,
            StatusCode::NOT_FOUND,
            "the grant names the work tools alone"
        );

        drop(grant);
        assert_eq!(
            initialize(&state, "elysium_work", Some(&token)).await,
            StatusCode::UNAUTHORIZED,
            "a finished turn's grant is revoked"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
    async fn the_coding_sessions_list_answers_with_each_sessions_activity() {
        let (url, _connection) = migrated_database().await;
        let state = app_state(&url).await;
        let cookie = signed_in_cookie(&state).await;

        let response = routes::router(&state)
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/coding-sessions")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .expect("a request builds"),
            )
            .await
            .expect("the router answers");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("the body reads");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
        assert_eq!(body, json!({ "sessions": [] }));
    }
}
