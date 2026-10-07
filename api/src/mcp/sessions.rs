// Copyright © 2026 Jalapeno Labs

//! Tools for coding sessions: an agent working on a project's repositories, in a thread on a
//! satellite.
//!
//! They call the coding session routes' own functions (`routes::v1::coding_sessions`), so a
//! session started here opens its thread, records its creator, and reaches the browser exactly
//! as one started in the Coding area does.
//!
//! A thread's history is thousands of small events, far more than a model should read at
//! once, so `sessions_events` answers the latest few of those Elysium renders, and a client
//! follows along by asking for what came after the last sequence it saw.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use validator::Validate;

use super::workspace::{Workspace, answer, caller, refuse_without_write};
use crate::errors::ApiError;
use crate::routes::v1::coding_sessions::{
    self, CreateCodingSessionRequest, EventWindow, Keep, RenameCodingSessionRequest,
    StartTurnRequest,
};

/// Events answered when a client does not say how many. Enough to read a turn's messages and
/// tool calls without filling a model's context.
const DEFAULT_EVENT_LIMIT: u32 = 100;

/// Names one session.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionArguments {
    /// The session's number, from `sessions_list`.
    id: i64,
}

/// A session and its new title.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameSessionArguments {
    /// The session's number, from `sessions_list`.
    id: i64,
    /// What the session is called, up to 200 characters.
    title: String,
}

/// A session and the prompt to send it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendPromptArguments {
    /// The session's number, from `sessions_list`.
    id: i64,
    /// What to ask the agent, up to 100,000 characters.
    prompt: String,
}

/// Which of a session's events to read.
#[derive(Debug, Deserialize, Validate, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionEventsArguments {
    /// The session's number, from `sessions_list`.
    id: i64,
    /// Only events after this sequence: pass the `nextSequence` of the previous answer to read
    /// on. Leave it out to read the latest events.
    after_sequence: Option<u64>,
    /// The most events answered, the latest kept when more match. Defaults to 100.
    #[validate(range(min = 1, max = 1000))]
    limit: Option<u32>,
}

#[tool_router(router = sessions_router, vis = "pub(super)")]
impl Workspace {
    #[tool(
        name = "sessions_list",
        description = "Lists the coding sessions, newest first: each one's number, title, \
            project, satellite, the action item it was started from, and its thread's latest \
            state (running, idle, waiting on a question, or ended)."
    )]
    async fn sessions_list(&self) -> Result<CallToolResult, ErrorData> {
        let sessions = coding_sessions::list(&self.state).await;
        answer(sessions.map(|sessions| json!({ "sessions": sessions })))
    }

    #[tool(
        name = "sessions_get",
        description = "Shows one coding session and its thread's latest state."
    )]
    async fn sessions_get(
        &self,
        Parameters(arguments): Parameters<SessionArguments>,
    ) -> Result<CallToolResult, ErrorData> {
        let session = coding_sessions::find(&self.state, arguments.id).await;
        answer(session.map(|session| json!({ "session": session })))
    }

    #[tool(
        name = "sessions_create",
        description = "Starts a coding session: opens a thread on a satellite for a project, \
            clones the repositories given, and, with a prompt, queues it as the agent's first \
            turn. A session started from an action item must have a prompt, and its first turn \
            carries the item's context. Find ids with projects_list and satellites_list. Needs \
            write access."
    )]
    async fn sessions_create(
        &self,
        Parameters(request): Parameters<CreateCodingSessionRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let caller = caller(&context)?;
        if let Some(refusal) = refuse_without_write(&caller) {
            return Ok(refusal);
        }
        let session = coding_sessions::create(&self.state, caller.user.id, request).await;
        answer(session.map(|session| json!({ "session": session })))
    }

    #[tool(
        name = "sessions_rename",
        description = "Renames a coding session. Needs write access."
    )]
    async fn sessions_rename(
        &self,
        Parameters(arguments): Parameters<RenameSessionArguments>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let caller = caller(&context)?;
        if let Some(refusal) = refuse_without_write(&caller) {
            return Ok(refusal);
        }
        let request = RenameCodingSessionRequest {
            title: arguments.title,
        };
        let session = coding_sessions::rename(&self.state, arguments.id, request).await;
        answer(session.map(|session| json!({ "session": session })))
    }

    #[tool(
        name = "sessions_send_prompt",
        description = "Sends a prompt to a coding session's agent as a new turn, queued behind \
            any turn still running. Answers the queued turn; follow its progress with \
            sessions_events. Needs write access."
    )]
    async fn sessions_send_prompt(
        &self,
        Parameters(arguments): Parameters<SendPromptArguments>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let caller = caller(&context)?;
        if let Some(refusal) = refuse_without_write(&caller) {
            return Ok(refusal);
        }
        let request = StartTurnRequest {
            prompt: arguments.prompt,
        };
        let turn = coding_sessions::start_turn(&self.state, arguments.id, request).await;
        answer(turn.map(|turn| json!({ "turn": turn })))
    }

    #[tool(
        name = "sessions_events",
        description = "Reads a coding session's conversation, oldest first: prompts, the \
            agent's messages and thinking, tool calls, plans, questions, finished turns, and \
            incidents. Without afterSequence it answers the latest events up to limit. With it, \
            it answers the events after that sequence, the earliest first up to limit, so no \
            event is skipped. Pass nextSequence back as afterSequence to read on; truncated says \
            more remain."
    )]
    async fn sessions_events(
        &self,
        Parameters(arguments): Parameters<SessionEventsArguments>,
    ) -> Result<CallToolResult, ErrorData> {
        if let Err(errors) = arguments.validate() {
            return answer(Err(ApiError::from(errors)));
        }
        // A first read wants what is happening now; a read from a cursor pages forward, so it
        // keeps the earliest events and skips none.
        let keep = match arguments.after_sequence {
            Some(_) => Keep::Earliest,
            None => Keep::Latest,
        };
        let window = EventWindow {
            after_sequence: arguments.after_sequence.unwrap_or(0),
            limit: arguments.limit.unwrap_or(DEFAULT_EVENT_LIMIT) as usize,
            keep,
            rendered_only: true,
        };
        let page = coding_sessions::read_events(&self.state, arguments.id, window).await;
        answer(page.map(|page| json!(page)))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_field_the_route_does_not_take_is_refused() {
        let unknown = serde_json::from_value::<RenameSessionArguments>(
            json!({ "id": 7, "title": "A", "x": 1 }),
        );
        assert!(
            unknown.is_err(),
            "a stray field is refused, as the route refuses one"
        );
    }

    #[test]
    fn an_event_limit_outside_its_range_is_refused() {
        let too_many: SessionEventsArguments =
            serde_json::from_value(json!({ "id": 7, "limit": 5000 })).expect("parses");
        assert!(too_many.validate().is_err());

        let none: SessionEventsArguments =
            serde_json::from_value(json!({ "id": 7, "limit": 0 })).expect("parses");
        assert!(none.validate().is_err());

        let default: SessionEventsArguments =
            serde_json::from_value(json!({ "id": 7 })).expect("parses");
        default.validate().expect("no limit takes the default");
    }
}
