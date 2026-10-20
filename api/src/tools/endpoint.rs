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
    CallToolRequestParams, CallToolResponse, CallToolResult, Content, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

use super::grants::{Granted, Grants};
#[cfg(doc)]
use super::grants::AGENT_TOOLS_PATH;
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
            .with_server_info(Implementation::new(self.server.name, env!("CARGO_PKG_VERSION")))
            .with_instructions(self.server.instructions)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
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
        Ok(ListToolsResult {
            tools,
            ..ListToolsResult::default()
        })
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
        let content = vec![Content::text(output.text)];
        let result = if output.is_error {
            CallToolResult::error(content)
        } else {
            CallToolResult::success(content)
        };
        Ok(result.into())
    }
}
