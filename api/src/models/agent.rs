// Copyright © 2026 Jalapeno Labs

//! Who makes a change to the knowledge base, as it is recorded beside the change.
//!
//! A learning, a page, and a proposal to the company doc all name the person or machine a change
//! is by and, when one acted for them, the agent ([`Author`], `docs/knowledge.md`). An agent is
//! recorded by what identifies it: `mcp:<client id>` for an MCP client a person connected,
//! `session:<number>` for a coding or Studio session's agent, and `project_manager:<project id>`
//! for a project's Project Manager. An MCP client is also labelled by the name it registered,
//! which is only what it calls itself.

use chrono::{DateTime, Utc};
use tracing::{Level, event};
use uuid::Uuid;

/// The agent acting for whoever made a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    /// Its recorded form, as the `agent` columns store it.
    id: String,
    /// What it called itself, such as `Claude Code`. Registered by the client, so a label only.
    name: Option<String>,
}

/// Who makes a change, and when.
#[derive(Debug, Clone, Copy)]
pub struct Author<'a> {
    /// The person, or the machine user standing for an agent of Elysium's own.
    pub user_id: Uuid,
    /// The agent acting for them, if one is.
    pub agent: Option<&'a Agent>,
    pub at: DateTime<Utc>,
}

/// The longest agent name kept, in characters, as the `agent_name` columns allow.
const AGENT_NAME_MAX_CHARACTERS: usize = 200;

/// The longest client id the `agent` columns hold after its `mcp:` prefix.
const CLIENT_ID_MAX_CHARACTERS: usize = 255;

impl Agent {
    /// The MCP client `client_id`, named as it registered itself.
    ///
    /// `None`, with a warning, for an id the columns cannot hold: Hydra issues ids far shorter,
    /// so only a client registered some other way could have one.
    pub fn mcp_client(client_id: &str, name: &str) -> Option<Self> {
        let length = client_id.chars().count();
        if length == 0
            || length > CLIENT_ID_MAX_CHARACTERS
            || client_id.contains(char::is_whitespace)
        {
            event!(
                name: "knowledge.agent.unrecordable",
                Level::WARN,
                oauth.client.id.length = length,
                "an MCP client id that cannot be recorded; the change names no agent",
            );
            return None;
        }
        let name = name.trim();
        let name =
            (!name.is_empty()).then(|| name.chars().take(AGENT_NAME_MAX_CHARACTERS).collect());
        Some(Self {
            id: format!("mcp:{client_id}"),
            name,
        })
    }

    /// The agent of the coding or Studio session with this number.
    pub fn session(number: i64) -> Self {
        Self {
            id: format!("session:{number}"),
            name: None,
        }
    }

    /// The Project Manager of the project with this id.
    pub fn project_manager(project_id: Uuid) -> Self {
        Self {
            id: format!("project_manager:{project_id}"),
            name: None,
        }
    }

    /// Its recorded form, as the `agent` columns store it.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// What it called itself, if anything.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elysiums_own_agents_are_recorded_by_what_identifies_them() {
        assert_eq!(Agent::session(12).id(), "session:12");
        let project_id = Uuid::nil();
        assert_eq!(
            Agent::project_manager(project_id).id(),
            format!("project_manager:{project_id}")
        );
    }

    #[test]
    fn an_mcp_client_is_recorded_by_its_id_and_labelled_by_its_name() {
        let agent = Agent::mcp_client("6b1f0c3e-77a4-4d0e-9a51-3c2d1f0e8a90", "  Claude Code ")
            .expect("an ordinary client id is recorded");
        assert_eq!(agent.id(), "mcp:6b1f0c3e-77a4-4d0e-9a51-3c2d1f0e8a90");
        assert_eq!(agent.name(), Some("Claude Code"));

        let unnamed = Agent::mcp_client("client", " ").expect("recorded");
        assert_eq!(unnamed.name(), None, "a blank name is no name");

        let long_name = Agent::mcp_client("client", &"n".repeat(500)).expect("recorded");
        assert_eq!(
            long_name.name().map(|name| name.chars().count()),
            Some(AGENT_NAME_MAX_CHARACTERS)
        );

        for unrecordable in ["", "has space", &"c".repeat(CLIENT_ID_MAX_CHARACTERS + 1)] {
            assert!(Agent::mcp_client(unrecordable, "Claude Code").is_none());
        }
    }
}
