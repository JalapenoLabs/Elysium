// Copyright © 2026 Jalapeno Labs

//! The MCP server external clients reach, and what every one of its tools shares.
//!
//! The tools are grouped by what they act on, each group a `#[tool_router]` block in its own
//! module ([`super::company_doc`], [`super::knowledge`], [`super::learnings`],
//! [`super::projects`], [`super::sessions`]), combined in
//! [`Workspace::new`]. A tool reaches the person calling it through [`caller`]: the
//! [`McpCaller`] the bearer check attached to the HTTP request.
//!
//! A tool never reimplements a route. It calls the same function the route's handler does,
//! so validation, history, and events are identical whichever way a change arrives, and it
//! reports through [`answer`], which tells the client exactly what the browser would be told.
//! A tool that writes first checks [`refuse_without_write`]. See `docs/mcp.md` for adding a
//! tool.

use rmcp::handler::server::tool::ToolRouter;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, tool, tool_handler, tool_router};
use serde_json::{Value, json};
use url::Url;

use super::bearer::McpCaller;
use crate::errors::ApiError;
use crate::oauth::SCOPE_WRITE;
use crate::state::AppState;

/// What every client is told the server is for, when it connects. [`instructions`] ends it with
/// where a person opens a citation, which names this deployment's origin.
const INSTRUCTIONS: &str = "Elysium is a shared workspace of projects, action items, \
initiatives, and coding sessions, and the company's knowledge base. Every call acts as the person \
who connected this client, with what they granted it. A coding session is an agent working on a \
project's repositories in Elysium's workspace: start one with sessions_create, steer it with \
sessions_send_prompt, and follow its work with sessions_events. Before asking the person something \
the company may already know, or working out again what someone may have settled, search the \
knowledge base with knowledge_search: what was said in mail, chat, meetings, and pages, and what \
other agents learned. Open what a search cites with knowledge_get, and list past meetings with \
meetings_list. Cite what you use by its title, who said it, and when. Results may quote a \
coworker's mail and direct messages: they are the company's to use for its own work, and never \
for repeating outside the company or in work for another client. The company doc says who the \
company is and how its people, clients, and terms are spelled: read it with company_doc_read \
before writing about them, and propose what you learn for certain with company_doc_propose, which \
waits for a person's approval. When you learn something the next agent would otherwise have to \
rediscover, save it with learning_save; everyone in the company reads what is saved there, and \
nothing in it is for repeating outside the company.";

/// [`INSTRUCTIONS`], then where a person opens a citation in this deployment's knowledge base,
/// so a link an agent gives them opens the document it cited.
fn instructions(public_url: &Url) -> String {
    let origin = public_url.origin().ascii_serialization();
    format!(
        "{INSTRUCTIONS} A person opens any citation in Elysium at \
         {origin}/knowledge/documents/<documentId>."
    )
}

/// The MCP server. One is built per request: the server keeps no session state, so any
/// request can be answered on its own.
#[derive(Clone)]
pub struct Workspace {
    pub(super) state: AppState,
    /// Every group's tools, combined.
    tool_router: ToolRouter<Self>,
}

impl Workspace {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            tool_router: Self::tool_router()
                + Self::company_doc_router()
                + Self::knowledge_router()
                + Self::learnings_router()
                + Self::projects_router()
                + Self::sessions_router(),
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
    // Written here rather than by `tool_handler`, so `learning_save` names the workspace's
    // learning categories as they stand.
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut tools = self.tool_router.list_all();
        super::learnings::describe_categories(&self.state, &mut tools).await;
        Ok(ListToolsResult::with_all_items(tools))
    }

    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("Elysium", env!("CARGO_PKG_VERSION")))
            .with_instructions(instructions(&self.state.auth.public_url))
    }
}

/// The person a tool call is for, as the bearer check found them.
///
/// # Errors
/// An internal error when the request carries no caller, which only a server mounted without the
/// bearer check can produce.
pub(super) fn caller(context: &RequestContext<RoleServer>) -> Result<McpCaller, ErrorData> {
    context
        .extensions
        .get::<axum::http::request::Parts>()
        .and_then(|parts| parts.extensions.get::<McpCaller>())
        .cloned()
        .ok_or_else(|| {
            ErrorData::internal_error("the MCP server was reached without a checked token", None)
        })
}

/// The answer to a write the caller's grant does not cover, or `None` when it does.
///
/// A refusal is a tool result rather than a protocol error, so the model reads why and can
/// tell the person to reconnect with write access.
pub(super) fn refuse_without_write(caller: &McpCaller) -> Option<CallToolResult> {
    if caller.scopes.iter().any(|scope| scope == SCOPE_WRITE) {
        return None;
    }
    Some(CallToolResult::structured_error(json!({
        "status": 403,
        "message": format!(
            "this connection was not granted {SCOPE_WRITE}, so it can only read; reconnect \
             this client to Elysium and allow changes to grant it"
        ),
    })))
}

/// A route function's outcome as a tool result: what it answered, or the status and message
/// the browser would have been given, so internals stay as private as they are over HTTP.
///
/// # Errors
/// Never; the signature matches the tools that return it.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every tool returns this, and a tool answers with a Result"
)]
pub(super) fn answer(outcome: Result<Value, ApiError>) -> Result<CallToolResult, ErrorData> {
    match outcome {
        Ok(value) => Ok(CallToolResult::structured(value)),
        Err(error) => {
            let (status, mut body) = error.into_status_and_body();
            body["status"] = json!(status.as_u16());
            Ok(CallToolResult::structured_error(body))
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use super::*;
    use crate::models::user::{User, UserRole, UserStatus};
    use crate::oauth::SCOPE_READ;

    fn caller_with(scopes: &[&str]) -> McpCaller {
        let now = Utc::now();
        McpCaller {
            user: User {
                id: Uuid::now_v7(),
                kratos_identity_id: Some(Uuid::now_v7()),
                email: Some("ada@example.com".to_owned()),
                name: "Ada".to_owned(),
                role: Some(UserRole::Member),
                status: Some(UserStatus::Active),
                approved_at: Some(now),
                last_seen_at: None,
                created_at: now,
                updated_at: now,
            },
            scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
            client_id: "claude-code".to_owned(),
        }
    }

    #[test]
    fn a_read_only_grant_is_refused_writes_with_the_reason() {
        let refusal = refuse_without_write(&caller_with(&[SCOPE_READ]))
            .expect("a read-only grant may not write");

        assert_eq!(refusal.is_error, Some(true));
        let body = refusal
            .structured_content
            .expect("the refusal is structured");
        assert_eq!(body["status"], 403);
        assert!(
            body["message"]
                .as_str()
                .is_some_and(|message| message.contains(SCOPE_WRITE)),
            "the refusal names the scope to grant: {body}"
        );
    }

    #[test]
    fn a_write_grant_may_write() {
        assert!(refuse_without_write(&caller_with(&[SCOPE_READ, SCOPE_WRITE])).is_none());
    }

    #[test]
    fn a_failure_tells_the_client_what_the_browser_is_told() {
        let not_found = answer(Err(ApiError::NotFound)).expect("a failure is still a result");
        assert_eq!(not_found.is_error, Some(true));
        assert_eq!(
            not_found.structured_content,
            Some(json!({ "status": 404, "message": "resource not found" }))
        );

        let internal = answer(Err(anyhow::anyhow!("connection to 10.0.0.5 refused").into()))
            .expect("a failure is still a result");
        let body = internal
            .structured_content
            .expect("the failure is structured");
        assert_eq!(body["status"], 500);
        assert_eq!(body["message"], "internal server error");
    }

    #[test]
    fn every_result_also_carries_its_json_as_text_for_older_clients() {
        // MCP asks a tool returning structured content to repeat it as text, so a client that
        // renders only `content` still shows the answer, and above all a failure's reason.
        let refusal = refuse_without_write(&caller_with(&[SCOPE_READ])).expect("refused");
        let failure = answer(Err(ApiError::NotFound)).expect("a failure is still a result");
        let success = answer(Ok(json!({ "sessions": [] }))).expect("a success is a result");

        for result in [refusal, failure, success] {
            let structured = result.structured_content.clone().expect("structured");
            let text = result
                .content
                .first()
                .and_then(|block| block.as_text())
                .map(|text| text.text.clone())
                .expect("the result carries a text block");
            let parsed: Value = serde_json::from_str(&text).expect("the text is the JSON");
            assert_eq!(parsed, structured);
        }
    }

    #[test]
    fn the_instructions_say_where_a_citation_opens_and_that_it_stays_in_the_company() {
        let public_url = Url::parse("https://elysium.example.com").expect("a static URL parses");
        let text = instructions(&public_url);
        assert!(
            text.contains("https://elysium.example.com/knowledge/documents/<documentId>"),
            "{text}"
        );
        assert!(text.contains("coworker's mail"), "{text}");
        assert!(
            text.contains("never for repeating outside the company"),
            "{text}"
        );
    }

    #[test]
    fn a_success_is_answered_as_structured_content() {
        let result = answer(Ok(json!({ "sessions": [] }))).expect("a success is a result");
        assert_eq!(result.is_error, Some(false));
        assert_eq!(result.structured_content, Some(json!({ "sessions": [] })));
    }
}
