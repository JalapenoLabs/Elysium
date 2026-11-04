// Copyright © 2026 Jalapeno Labs

//! The MCP client acting for a person, as the knowledge base records it beside their change.
//!
//! A learning and a proposal to the company doc both name the person a change is by and, when
//! one acted for them, the client they connected (`docs/knowledge.md`). The client is recorded
//! by its id, `mcp:<client id>`, which is its identity, and labelled by the name it registered,
//! which is only what it calls itself.

use tracing::{Level, event};

/// The agent acting for the person who made a change: today an MCP client they connected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    /// Its recorded form, `mcp:<client id>`.
    id: String,
    /// What it called itself, such as `Claude Code`. Registered by the client, so a label only.
    name: Option<String>,
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

    /// Its recorded form, `mcp:<client id>`, as the `agent` columns store it.
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
