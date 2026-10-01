// Copyright © 2026 Jalapeno Labs

//! The tools Elysium's MCP server offers external clients.
//!
//! Each tool is a method in the `#[tool_router]` block below, and reaches the person calling it
//! through [`caller`]: the [`McpCaller`] the bearer check attached to the HTTP request. A tool
//! that writes must also find `workspace:write` in the caller's scopes, and records the person as
//! the actor, exactly as the routes do for the browser. See `docs/mcp.md` for adding a tool.

use rmcp::handler::server::tool::ToolRouter;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, tool, tool_handler, tool_router};

use super::bearer::McpCaller;

/// What every client is told the server is for, when it connects.
const INSTRUCTIONS: &str = "Elysium is a shared workspace of projects, action items, \
initiatives, and coding sessions. Every call acts as the person who connected this client, with \
what they granted it.";

/// The MCP server. One is built per request: the server keeps no session state, so any
/// request can be answered on its own.
#[derive(Debug, Clone)]
pub struct Workspace {
    tool_router: ToolRouter<Self>,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl Workspace {
    /// Says who the client is connected as. The smallest call that proves a connection works.
    #[tool(
        name = "hello",
        description = "Say hello, and show who this connection acts as in Elysium."
    )]
    async fn hello(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let caller = caller(&context)?;
        let user = &caller.user;
        let greeting = format!(
            "Hello, {}! This connection acts as {} in Elysium, through OAuth client {}, with {}.",
            user.name,
            user.email.as_deref().unwrap_or("an Elysium user"),
            caller.client_id,
            caller.scopes.join(", "),
        );
        Ok(CallToolResult::success(vec![ContentBlock::text(greeting)]))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Workspace {
    // Written here rather than by `tool_handler`, whose async version has nothing to await.
    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(
            self.tool_router.list_all(),
        )))
    }

    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("Elysium", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}

/// The person a tool call is for, as the bearer check found them.
///
/// # Errors
/// An internal error when the request carries no caller, which only a server mounted without the
/// bearer check can produce.
fn caller(context: &RequestContext<RoleServer>) -> Result<McpCaller, ErrorData> {
    context
        .extensions
        .get::<axum::http::request::Parts>()
        .and_then(|parts| parts.extensions.get::<McpCaller>())
        .cloned()
        .ok_or_else(|| {
            ErrorData::internal_error("the MCP server was reached without a checked token", None)
        })
}
