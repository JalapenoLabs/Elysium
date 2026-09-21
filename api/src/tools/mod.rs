// Copyright © 2026 Jalapeno Labs

//! Tools Elysium serves to coding agents, relayed through their satellites.
//!
//! A satellite cannot reach Elysium, so an agent's MCP tools that need Elysium's data are
//! declared on the thread as relayed MCP servers. The agent calls them like any MCP tool;
//! the satellite forwards each call over a socket Elysium opened to it (see
//! `crate::fleet::relay`), and Elysium answers there.
//!
//! Each server is a [`ToolServer`]: its name, the instructions the agent reads, and a table
//! of [`Tool`]s. The same table declares the tools on a new thread and dispatches a call to
//! them, so a declared tool always has a handler. Each tool set is one module with its own
//! table, listed in [`SERVERS`]: [`storage`] for the project's storage locations and
//! [`work`] for its action items and initiatives.
//!
//! Results are JSON text. A failure answers the agent with what to fix, never with an
//! internal error chain: database and decryption failures are logged here and reported
//! as a fixed message.

pub mod storage;
pub mod work;

use std::sync::Arc;

use arsox_sdk::client::ThreadHandle;
use arsox_sdk::proto::settings::v1::{RelayedMcpServer, RelayedTool};
use futures_util::future::BoxFuture;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tracing::{Level, event};
use uuid::Uuid;

use crate::action_items::links::Links;
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::realtime::EventBus;
use crate::storage::Storage;

/// The services tool calls use. Cheap to clone; clones share every connection.
#[derive(Clone)]
pub struct ToolContext {
    pub database: Pool,
    pub cipher: Arc<Cipher>,
    pub storage: Storage,
    /// Where a call that writes tells every client about the change.
    pub events: EventBus,
    /// Links to Jira and GitHub, for the tool that links a session's pull request.
    pub links: Links,
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

/// Who a call is made for: the coding session whose agent called, its project, which
/// decides what the call may reach, the action item it was started from, and its thread,
/// whose workspace files the call may read and write.
#[derive(Clone)]
pub struct CallScope {
    pub session_id: i64,
    pub project_id: Uuid,
    pub action_item_id: Option<Uuid>,
    pub workspace: ThreadHandle,
}

/// Written by hand because the SDK's derived `Debug` for a thread handle prints the
/// satellite's bearer secret; only the thread's id is shown.
impl std::fmt::Debug for CallScope {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CallScope")
            .field("session_id", &self.session_id)
            .field("project_id", &self.project_id)
            .field("action_item_id", &self.action_item_id)
            .field("thread_id", &self.workspace.id())
            .finish()
    }
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
    /// 1 to 64 letters, digits, underscores, or hyphens, not starting with `arsox`.
    pub name: &'static str,
    pub instructions: &'static str,
    pub tools: &'static [Tool],
}

/// Every server Elysium can relay, looked up by name when a call arrives.
pub const SERVERS: [&ToolServer; 2] = [&storage::SERVER, &work::SERVER];

/// A server as a thread declares it, so the satellite serves its tools to the agent.
pub fn relayed_server(server: &ToolServer) -> RelayedMcpServer {
    RelayedMcpServer {
        name: server.name.to_owned(),
        instructions: server.instructions.to_owned(),
        tools: server
            .tools
            .iter()
            .map(|tool| RelayedTool {
                name: tool.name.to_owned(),
                description: tool.description.to_owned(),
                input_schema_json: (tool.input_schema)().to_string(),
            })
            .collect(),
    }
}

/// Why a call could not be carried out. `Display` is what the agent reads, so every
/// message says what to change.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    /// No server or tool by that name is served for this session.
    #[error("{0}")]
    Unknown(String),
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
    /// The storage provider refused or could not be reached, in its own words.
    #[error("{0}")]
    Provider(String),
    /// The satellite could not read or write the workspace file.
    #[error("the workspace file could not be transferred: {0}")]
    Workspace(String),
    /// Elysium itself failed; the details are logged, never sent to the agent.
    #[error("Elysium could not complete the call; try again later")]
    Internal,
}

/// A call's answer, ready for the relay.
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
            session.id = %scope.session_id,
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
        session.id = %scope.session_id,
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

    /// The satellite's rule for relayed server and tool names.
    fn is_relay_name(name: &str) -> bool {
        (1..=64).contains(&name.len())
            && name.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
            })
    }

    #[test]
    fn every_server_and_tool_has_a_name_the_satellite_accepts() {
        let mut server_names = HashSet::new();
        for server in SERVERS {
            assert!(is_relay_name(server.name), "{}", server.name);
            assert!(!server.name.starts_with("arsox"), "{}", server.name);
            assert!(
                server_names.insert(server.name),
                "{} is listed twice",
                server.name
            );
            assert!(!server.instructions.trim().is_empty());

            let mut tool_names = HashSet::new();
            for tool in server.tools {
                assert!(is_relay_name(tool.name), "{}", tool.name);
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
