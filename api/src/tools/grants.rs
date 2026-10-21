// Copyright © 2026 Jalapeno Labs

//! Per-turn access to Elysium's tool servers.
//!
//! Before a turn starts, its caller mints a [`Grant`] naming the call scope and the servers
//! the turn may use. The grant's bearer token goes into the turn's MCP configuration, and
//! every request to `/api/agent-tools/{server}` must present it (see `super::endpoint`). The
//! token is 256 random bits; only its SHA-256 is held, so the map never contains a usable
//! credential. Dropping the grant, which the caller does when the turn finishes, revokes it.
//!
//! Grants live in memory. The API runs as one replica, and a restart ends every turn, so a
//! grant could never outlive what it was for anyway.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest as _, Sha256};
use tracing::{Level, event};
use url::Url;

use super::{CallScope, ToolServer};
use crate::runner::{AGENT_API_URL, McpServer};

/// Where the tool servers are mounted, under [`AGENT_API_URL`].
pub const AGENT_TOOLS_PATH: &str = "/api/agent-tools";

/// What a grant allows: a scope, and the servers it may call.
#[derive(Debug, Clone)]
pub struct Granted {
    pub scope: CallScope,
    pub servers: Vec<&'static ToolServer>,
}

/// Every live grant, by the SHA-256 of its token. Cheap to clone; clones share the map.
#[derive(Debug, Clone, Default)]
pub struct Grants {
    live: Arc<Mutex<HashMap<[u8; 32], Granted>>>,
}

/// A grant held for one turn. Dropping it revokes the token.
#[derive(Debug)]
pub struct Grant {
    grants: Grants,
    digest: [u8; 32],
    token: SecretString,
    servers: Vec<&'static ToolServer>,
}

impl Grants {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mints a grant for `scope` to call `servers`.
    pub fn grant(&self, scope: CallScope, servers: &[&'static ToolServer]) -> Grant {
        let mut random = [0_u8; 32];
        getrandom::fill(&mut random).expect("the operating system provides randomness");
        let token = hex::encode(random);
        let digest = digest(&token);
        lock(&self.live).insert(
            digest,
            Granted {
                scope,
                servers: servers.to_vec(),
            },
        );
        Grant {
            grants: self.clone(),
            digest,
            token: SecretString::from(token),
            servers: servers.to_vec(),
        }
    }

    /// What `token` grants, if it is live.
    pub fn find(&self, token: &str) -> Option<Granted> {
        lock(&self.live).get(&digest(token)).cloned()
    }
}

impl Grant {
    /// The turn's MCP servers, one per granted server, each with the bearer header.
    pub fn mcp_servers(&self) -> Vec<McpServer> {
        self.servers
            .iter()
            .map(|server| McpServer::Http {
                name: server.name.to_owned(),
                url: Url::parse(&format!(
                    "{AGENT_API_URL}{AGENT_TOOLS_PATH}/{}",
                    server.name
                ))
                .expect("server names are URL-safe"),
                headers: vec![(
                    "Authorization".to_owned(),
                    SecretString::from(format!("Bearer {}", self.token.expose_secret())),
                )],
            })
            .collect()
    }
}

impl Drop for Grant {
    fn drop(&mut self) {
        if lock(&self.grants.live).remove(&self.digest).is_none() {
            event!(
                name: "tools.grant.already_revoked",
                Level::DEBUG,
                "a grant was revoked twice",
            );
        }
    }
}

fn digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// Every critical section leaves the map whole, so a poisoned lock holds nothing half done.
fn lock<Value>(mutex: &Mutex<Value>) -> std::sync::MutexGuard<'_, Value> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::WorkspacePath;
    use crate::tools::{SessionScope, work};

    fn scope() -> CallScope {
        CallScope::Session(SessionScope {
            session_id: 7,
            project_id: None,
            action_item_id: None,
            directory: WorkspacePath::new("sessions/7").expect("a path"),
        })
    }

    #[test]
    fn a_grant_works_until_it_is_dropped() {
        let grants = Grants::new();
        let grant = grants.grant(scope(), &[&work::SERVER]);
        let servers = grant.mcp_servers();
        let McpServer::Http { url, headers, .. } = &servers[0] else {
            panic!("the tools are served over HTTP");
        };
        assert_eq!(url.as_str(), "http://api:8080/api/agent-tools/elysium_work");
        let token = headers[0]
            .1
            .expose_secret()
            .strip_prefix("Bearer ")
            .expect("a bearer")
            .to_owned();

        let found = grants.find(&token).expect("live");
        assert_eq!(found.servers.len(), 1);
        assert!(grants.find("not-a-token").is_none());

        drop(grant);
        assert!(grants.find(&token).is_none(), "dropping revokes it");
    }
}
