// Copyright © 2026 Jalapeno Labs

//! Tools Elysium serves to agents: MCP servers that answer from Elysium's own data.
//!
//! Agents run in the workspace container and reach these servers over HTTP at
//! `/api/agent-tools/{server}` ([`endpoint`]), with a bearer token their turn was granted
//! ([`grants`]). The grant carries the [`CallScope`] every call runs for.
//!
//! Each server is a [`ToolServer`]: its name, the instructions the agent reads (fixed in code, or
//! for [`knowledge`] the company's agent guidance), and a table of [`Tool`]s. The same table lists the tools to the agent and dispatches a call to them,
//! so a listed tool always has a handler. Each tool set is one module with its own
//! table, listed in [`SERVERS`]: [`storage`] for the project's storage locations, for coding
//! sessions; [`work`] for its action items and initiatives, for coding sessions and Project
//! Managers; [`elysia`] for Elysia's own turns; [`project_manager`] for a Project Manager's
//! own; [`worker`] for a pool worker's own; [`passwords`] for the logins any agent may sign in
//! with; and [`knowledge`] for the company's knowledge base, which every agent reads.
//!
//! Results are JSON text. A failure answers the agent with what to fix, never with an
//! internal error chain: database and decryption failures are logged here and reported
//! as a fixed message.

pub mod elysia;
pub mod endpoint;
pub mod grants;
pub mod knowledge;
pub mod passwords;
pub mod project_manager;
pub mod storage;
pub mod work;
pub mod worker;

use std::sync::Arc;

use futures_util::future::BoxFuture;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tracing::{Level, event};
use uuid::Uuid;

use crate::action_items::links::Links;
use crate::agent_guidance::{AgentGuidance, Audience};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::github::Github;
use crate::knowledge::search::KnowledgeSearch;
use crate::passwords::remote::RemoteVault;
use crate::realtime::EventBus;
use crate::runner::WorkspacePath;
use crate::runner::workspace::WorkspaceContainer;
use crate::storage::Storage;

/// The services tool calls use. Cheap to clone; clones share every connection.
#[derive(Clone)]
pub struct ToolContext {
    /// The guidance a server that serves it adds to its instructions as a turn connects.
    pub agent_guidance: AgentGuidance,
    pub database: Pool,
    pub cipher: Arc<Cipher>,
    pub storage: Storage,
    /// Where a call that writes tells every client about the change.
    pub events: EventBus,
    /// Links to Jira and GitHub, for the tool that links a session's pull request.
    pub links: Links,
    /// GitHub, for the tool a worker records its pull requests with.
    pub github: Github,
    /// Where a session's files are read and written.
    pub workspace: WorkspaceContainer,
    /// The password vault, for the logins agents sign in with.
    pub passwords: RemoteVault,
    /// The knowledge base's search, whose budgets agents' searches spend as people's do.
    pub knowledge: KnowledgeSearch,
}

impl std::fmt::Debug for ToolContext {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The pool and cipher have nothing useful to print.
        formatter
            .debug_struct("ToolContext")
            .field("storage", &self.storage)
            .finish_non_exhaustive()
    }
}

/// Who a call is made for. Each server's tools accept the scopes they serve and answer any
/// other with what the tool is for. Stored with its grant (`super::grants`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CallScope {
    /// A coding or Studio session's agent.
    Session(SessionScope),
    /// One of Elysia's turns. She acts for the whole workspace, so nothing narrows her reach.
    Elysia(ElysiaScope),
    /// One turn of a project's Project Manager, which reaches its own project alone.
    ProjectManager(ProjectManagerScope),
    /// One turn of a pool worker, which reaches its own batch alone.
    Worker(WorkerScope),
}

impl CallScope {
    /// The session a session's tool runs for.
    ///
    /// # Errors
    /// Returns [`ToolError::WrongAgent`] for Elysia, whose turns have tools of their own.
    pub fn session(&self) -> Result<&SessionScope, ToolError> {
        match self {
            Self::Session(session) => Ok(session),
            Self::Elysia(_) | Self::ProjectManager(_) | Self::Worker(_) => {
                Err(ToolError::WrongAgent("coding sessions"))
            }
        }
    }

    /// The turn one of Elysia's tools runs for.
    ///
    /// # Errors
    /// Returns [`ToolError::WrongAgent`] for a coding session.
    pub fn elysia(&self) -> Result<&ElysiaScope, ToolError> {
        match self {
            Self::Elysia(elysia) => Ok(elysia),
            Self::Session(_) | Self::ProjectManager(_) | Self::Worker(_) => {
                Err(ToolError::WrongAgent("Elysia"))
            }
        }
    }

    /// The turn one of a Project Manager's tools runs for.
    ///
    /// # Errors
    /// Returns [`ToolError::WrongAgent`] for anyone else.
    pub fn project_manager(&self) -> Result<&ProjectManagerScope, ToolError> {
        match self {
            Self::ProjectManager(manager) => Ok(manager),
            Self::Session(_) | Self::Elysia(_) | Self::Worker(_) => {
                Err(ToolError::WrongAgent("Project Managers"))
            }
        }
    }

    /// The turn one of a pool worker's tools runs for.
    ///
    /// # Errors
    /// Returns [`ToolError::WrongAgent`] for anyone else.
    pub fn worker(&self) -> Result<&WorkerScope, ToolError> {
        match self {
            Self::Worker(worker) => Ok(worker),
            Self::Session(_) | Self::Elysia(_) | Self::ProjectManager(_) => {
                Err(ToolError::WrongAgent("the worker pool's workers"))
            }
        }
    }

    /// Who called, for logs: `session:<n>`, `elysia:<turn>`, `project_manager:<turn>`, or
    /// `worker:<batch>`.
    pub fn label(&self) -> String {
        match self {
            Self::Session(session) => format!("session:{}", session.session_id),
            Self::Elysia(elysia) => format!("elysia:{}", elysia.turn_id),
            Self::ProjectManager(manager) => format!("project_manager:{}", manager.turn_id),
            Self::Worker(worker) => format!("worker:{}", worker.batch_id),
        }
    }
}

/// One turn of a pool worker: the batch it runs, and that batch's project.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerScope {
    pub batch_id: Uuid,
    pub project_id: Uuid,
}

/// One turn of a project's Project Manager: the project it plans, and the turn, which what it
/// submits names as its provenance.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectManagerScope {
    pub project_id: Uuid,
    pub turn_id: Uuid,
}

/// One of Elysia's turns.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElysiaScope {
    pub turn_id: Uuid,
}

/// A session whose agent called: its project, which decides what the call may reach, the
/// action item it was started from, and its directory, whose files the call may read and
/// write.
///
/// A Studio session may have no project; it then reaches only the storage locations for
/// every project, and no work at all.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionScope {
    pub session_id: i64,
    pub project_id: Option<Uuid>,
    pub action_item_id: Option<Uuid>,
    pub directory: WorkspacePath,
}

/// One tool: what the agent is told about it, and what runs when it is called.
#[derive(Debug, Clone, Copy)]
pub struct Tool {
    /// 1 to 64 letters, digits, underscores, or hyphens, unique within its server.
    pub name: &'static str,
    pub description: &'static str,
    /// The JSON Schema the arguments must match, built on demand.
    pub input_schema: fn() -> Value,
    pub run: Run,
}

/// Runs a call with its raw JSON arguments.
pub type Run = for<'call> fn(
    &'call ToolContext,
    &'call CallScope,
    &'call str,
) -> BoxFuture<'call, Result<Value, ToolError>>;

/// A relayed MCP server: a named set of tools with the instructions that explain them.
#[derive(Debug, Clone, Copy)]
pub struct ToolServer {
    /// 1 to 64 letters, digits, underscores, or hyphens: an MCP server name and a URL segment.
    pub name: &'static str,
    pub instructions: Instructions,
    pub tools: &'static [Tool],
}

/// What a turn is told a server is for, as it connects.
#[derive(Debug, Clone, Copy)]
pub enum Instructions {
    /// Text fixed in code.
    Fixed(&'static str),
    /// The company's agent guidance, as `audience` reads it, current as the turn connects
    /// (`crate::agent_guidance`).
    Guidance(Audience),
}

/// Every server Elysium serves, looked up by name when a call arrives.
pub const SERVERS: [&ToolServer; 7] = [
    &storage::SERVER,
    &work::SERVER,
    &elysia::SERVER,
    &project_manager::SERVER,
    &worker::SERVER,
    &passwords::SERVER,
    &knowledge::SERVER,
];

/// Why a call could not be carried out. `Display` is what the agent reads, so every
/// message says what to change.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    /// No server or tool by that name is served for this session.
    #[error("{0}")]
    Unknown(String),
    /// The tool belongs to another kind of agent, named here.
    #[error("this tool is for {0}; use the tools your own server lists")]
    WrongAgent(&'static str),
    /// The arguments, or what they name, cannot be used as given.
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    NotFound(String),
    #[error(
        "storage location {0} is not available to this session's project; call \
         storage_locations for the ones that are"
    )]
    LocationUnavailable(Uuid),
    #[error(
        "action item {0} is not in this session's project or was deleted; call work_items for \
         the ones that are"
    )]
    ItemUnavailable(Uuid),
    #[error(
        "initiative {0} is not in this session's project or was deleted; call \
         work_initiatives for the ones that are"
    )]
    InitiativeUnavailable(Uuid),
    /// Elysium declined the call, saying why and when to try again: the knowledge base is for
    /// the company's members alone, a search budget is spent for now, or a page refuses the write.
    #[error("{0}")]
    Refused(String),
    /// The storage provider refused or could not be reached, in its own words.
    #[error("{0}")]
    Provider(String),
    /// The workspace file could not be read or written.
    #[error("the workspace file could not be transferred: {0}")]
    Workspace(String),
    /// Elysium itself failed; the details are logged, never sent to the agent.
    #[error("Elysium could not complete the call; try again later")]
    Internal,
}

/// A call's answer, ready for the endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    pub text: String,
    pub is_error: bool,
}

impl From<Result<Value, ToolError>> for ToolOutput {
    fn from(result: Result<Value, ToolError>) -> Self {
        match result {
            Ok(value) => Self {
                text: value.to_string(),
                is_error: false,
            },
            Err(error) => Self {
                text: error.to_string(),
                is_error: true,
            },
        }
    }
}

/// Runs the tool `tool` of server `server` for `scope`.
pub async fn dispatch(
    context: &ToolContext,
    scope: &CallScope,
    server: &str,
    tool: &str,
    arguments_json: &str,
) -> ToolOutput {
    let Some(found) = find_tool(server, tool) else {
        event!(
            name: "tools.call.unknown",
            Level::WARN,
            tool.caller = scope.label(),
            tool.server = server,
            tool.name = tool,
            "a call named a tool Elysium does not serve",
        );
        let unknown = ToolError::Unknown(format!(
            "Elysium serves no tool named {tool} on the server {server}"
        ));
        return Err(unknown).into();
    };
    (found.run)(context, scope, arguments_json).await.into()
}

fn find_tool(server: &str, tool: &str) -> Option<&'static Tool> {
    SERVERS
        .iter()
        .find(|candidate| candidate.name == server)?
        .tools
        .iter()
        .find(|candidate| candidate.name == tool)
}

/// Reads a call's arguments into `Arguments`. An empty string counts as no arguments.
///
/// # Errors
/// Returns [`ToolError::Invalid`] naming what does not match the tool's schema.
pub fn parse_arguments<Arguments: DeserializeOwned>(
    arguments_json: &str,
) -> Result<Arguments, ToolError> {
    let arguments_json = match arguments_json.trim() {
        "" => "{}",
        trimmed => trimmed,
    };
    serde_json::from_str(arguments_json).map_err(|error| {
        ToolError::Invalid(format!(
            "the arguments do not match the tool's input schema: {error}"
        ))
    })
}

/// Logs an internal failure with its detail and hands the agent [`ToolError::Internal`].
pub fn internal(
    scope: &CallScope,
    operation: &'static str,
    error: &dyn std::fmt::Display,
) -> ToolError {
    event!(
        name: "tools.call.internal_failure",
        Level::ERROR,
        tool.caller = scope.label(),
        tool.operation = operation,
        error.message = %error,
        "a tool call failed inside Elysium",
    );
    ToolError::Internal
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    /// MCP clients, Claude Code among them, accept tool and server names of this shape, and
    /// a server's name is a URL path segment.
    fn is_tool_name(name: &str) -> bool {
        (1..=64).contains(&name.len())
            && name.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
            })
    }

    #[test]
    fn every_server_and_tool_has_a_name_clients_accept() {
        let mut server_names = HashSet::new();
        for server in SERVERS {
            assert!(is_tool_name(server.name), "{}", server.name);
            assert!(
                server_names.insert(server.name),
                "{} is listed twice",
                server.name
            );
            if let Instructions::Fixed(instructions) = server.instructions {
                assert!(!instructions.trim().is_empty(), "{}", server.name);
            }

            let mut tool_names = HashSet::new();
            for tool in server.tools {
                assert!(is_tool_name(tool.name), "{}", tool.name);
                assert!(
                    tool_names.insert(tool.name),
                    "{} is listed twice",
                    tool.name
                );
                assert!(!tool.description.trim().is_empty(), "{}", tool.name);
            }
        }
    }

    #[test]
    fn every_schema_is_a_closed_object() {
        for server in SERVERS {
            for tool in server.tools {
                let schema = (tool.input_schema)();
                assert_eq!(schema["type"], "object", "{}", tool.name);
                assert_eq!(schema["additionalProperties"], false, "{}", tool.name);
                let properties = schema["properties"]
                    .as_object()
                    .unwrap_or_else(|| panic!("{} lists its properties", tool.name));
                let required = schema["required"].as_array().map_or(&[][..], Vec::as_slice);
                for name in required {
                    let name = name.as_str().expect("required names are strings");
                    assert!(properties.contains_key(name), "{}: {name}", tool.name);
                }
            }
        }
    }

    /// Only a person decides on a proposed work batch. The boundary is the type:
    /// `crate::worker_pool::actions::approve` and `reject` take a `MemberUser`, which no machine
    /// user is. This is a tripwire beside it, by name, so a tool meant to decide is noticed in
    /// review; it cannot prove a tool by another name does not.
    #[test]
    fn no_agent_tool_decides_on_a_proposed_batch() {
        for server in SERVERS {
            for tool in server.tools {
                assert!(
                    !tool.name.contains("approve") && !tool.name.contains("reject"),
                    "{} on {} may not decide on a proposal",
                    tool.name,
                    server.name
                );
            }
        }
    }

    #[test]
    fn every_declared_tool_is_dispatched() {
        for server in SERVERS {
            for tool in server.tools {
                let found = find_tool(server.name, tool.name).expect("dispatchable");
                assert_eq!(found.name, tool.name);
            }
        }
        assert!(find_tool("elysium_storage", "storage_rename").is_none());
        assert!(find_tool("elysium_mail", "storage_list").is_none());
        assert!(
            find_tool("elysium_storage", "work_items").is_none(),
            "a tool is found only on its own server"
        );
    }

    #[test]
    fn outputs_carry_json_or_the_message_to_act_on() {
        let answered = ToolOutput::from(Ok(serde_json::json!({ "deleted": "a.txt" })));
        assert_eq!(answered.text, r#"{"deleted":"a.txt"}"#);
        assert!(!answered.is_error);

        let refused = ToolOutput::from(Err(ToolError::LocationUnavailable(Uuid::nil())));
        assert!(refused.is_error);
        assert!(
            refused.text.contains("storage_locations"),
            "{}",
            refused.text
        );

        let item = ToolOutput::from(Err(ToolError::ItemUnavailable(Uuid::nil())));
        assert!(item.is_error);
        assert!(item.text.contains("work_items"), "{}", item.text);
        let initiative = ToolOutput::from(Err(ToolError::InitiativeUnavailable(Uuid::nil())));
        assert!(
            initiative.text.contains("work_initiatives"),
            "{}",
            initiative.text
        );

        let internal = ToolOutput::from(Err(ToolError::Internal));
        assert!(!internal.text.contains("database"), "{}", internal.text);
    }

    #[test]
    fn empty_arguments_read_as_an_empty_object() {
        #[expect(
            clippy::empty_structs_with_brackets,
            reason = "serde reads {} only into a braced struct; a unit struct takes null"
        )]
        #[derive(Debug, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Nothing {}

        parse_arguments::<Nothing>("").expect("empty");
        parse_arguments::<Nothing>(" {} ").expect("an empty object");
        let unknown = parse_arguments::<Nothing>(r#"{"extra":1}"#).expect_err("unknown field");
        assert!(matches!(unknown, ToolError::Invalid(message) if message.contains("extra")));
    }
}
