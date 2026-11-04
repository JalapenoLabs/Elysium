// Copyright © 2026 Jalapeno Labs

//! Per-turn access to Elysium's tool servers.
//!
//! Before a turn starts, the engine mints a [`Grant`] naming the call scope and the servers the
//! turn may use. The grant's bearer token goes into the turn's MCP configuration, and every
//! request to `/api/agent-tools/{server}`, which the API answers, must present it (see
//! `super::endpoint`).
//!
//! Grants live in Postgres (`agent_tool_grants`), because the engine mints them and the API
//! checks them, and any number of API replicas may answer a call. The token is 256 random bits,
//! and only its SHA-256 is stored, so the table never holds a usable credential. The engine
//! revokes a grant when its turn ends ([`Grant::revoke`]), and all of them when it starts
//! ([`Grants::revoke_all`]), since a restart of the engine has ended every turn.

use anyhow::Context as _;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest as _, Sha256};
use tracing::{Level, event};
use url::Url;
use uuid::Uuid;

use super::{CallScope, SERVERS, ToolServer};
use crate::database::Pool;
use crate::database::schema::agent_tool_grants;
use crate::models::user::{CODING_AGENT_USER_ID, ELYSIA_USER_ID, PROJECT_MANAGER_USER_ID};
use crate::runner::{AGENT_API_URL, McpServer};

/// Where the tool servers are mounted, under [`AGENT_API_URL`].
pub const AGENT_TOOLS_PATH: &str = "/api/agent-tools";

/// What a grant allows: a scope, and the servers it may call.
#[derive(Debug, Clone)]
pub struct Granted {
    pub scope: CallScope,
    pub servers: Vec<&'static ToolServer>,
}

/// Every live grant, in Postgres. Cheap to clone; clones share the pool.
#[derive(Clone)]
pub struct Grants {
    database: Pool,
}

impl std::fmt::Debug for Grants {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Grants").finish_non_exhaustive()
    }
}

/// A grant held for one turn. [`Grant::revoke`] ends it; dropping it unrevoked, such as when a
/// turn's follower panics, revokes it in the background.
#[derive(Debug)]
pub struct Grant {
    grants: Grants,
    digest: [u8; 32],
    token: SecretString,
    servers: Vec<&'static ToolServer>,
    revoked: bool,
}

#[derive(Insertable)]
#[diesel(table_name = agent_tool_grants)]
struct NewGrant<'a> {
    token_sha256: &'a [u8],
    scope: serde_json::Value,
    servers: Vec<&'static str>,
    created_by: Uuid,
}

impl Grants {
    pub const fn new(database: Pool) -> Self {
        Self { database }
    }

    /// Mints a grant for `scope` to call `servers`, made by the agent the scope names.
    ///
    /// # Errors
    /// Fails when the grant cannot be stored.
    pub async fn grant(
        &self,
        scope: &CallScope,
        servers: &[&'static ToolServer],
    ) -> anyhow::Result<Grant> {
        let mut random = [0_u8; 32];
        getrandom::fill(&mut random).expect("the operating system provides randomness");
        let token = hex::encode(random);
        let digest = digest(&token);
        let created_by = match scope {
            CallScope::Session(_) => CODING_AGENT_USER_ID,
            CallScope::Elysia(_) => ELYSIA_USER_ID,
            CallScope::ProjectManager(_) => PROJECT_MANAGER_USER_ID,
        };

        let mut connection = self
            .database
            .get()
            .await
            .context("no database connection to store a tool grant")?;
        diesel::insert_into(agent_tool_grants::table)
            .values(NewGrant {
                token_sha256: &digest,
                scope: serde_json::to_value(scope).expect("a call scope serializes"),
                servers: servers.iter().map(|server| server.name).collect(),
                created_by,
            })
            .execute(&mut connection)
            .await
            .context("cannot store a tool grant")?;
        Ok(Grant {
            grants: self.clone(),
            digest,
            token: SecretString::from(token),
            servers: servers.to_vec(),
            revoked: false,
        })
    }

    /// What `token` grants, if it is live. A stored server this build does not serve is left
    /// out, and a scope it cannot read refuses the grant.
    ///
    /// # Errors
    /// Fails when the database cannot be read.
    pub async fn find(&self, token: &str) -> anyhow::Result<Option<Granted>> {
        let mut connection = self
            .database
            .get()
            .await
            .context("no database connection to check a tool grant")?;
        let stored: Option<(serde_json::Value, Vec<Option<String>>)> = agent_tool_grants::table
            .find(digest(token).as_slice())
            .select((agent_tool_grants::scope, agent_tool_grants::servers))
            .first(&mut connection)
            .await
            .optional()
            .context("cannot read a tool grant")?;
        let Some((scope, names)) = stored else {
            return Ok(None);
        };
        let scope: CallScope = match serde_json::from_value(scope) {
            Ok(scope) => scope,
            Err(error) => {
                event!(
                    name: "tools.grant.unreadable",
                    Level::WARN,
                    error.message = %error,
                    "a stored tool grant's scope could not be read; refusing it",
                );
                return Ok(None);
            }
        };
        let servers = SERVERS
            .into_iter()
            .filter(|server| names.iter().flatten().any(|name| name == server.name))
            .collect();
        Ok(Some(Granted { scope, servers }))
    }

    /// Revokes every grant, for an engine starting: whatever turns they were for have ended.
    ///
    /// # Errors
    /// Fails when the database cannot be written.
    pub async fn revoke_all(&self) -> anyhow::Result<usize> {
        let mut connection = self
            .database
            .get()
            .await
            .context("no database connection to revoke tool grants")?;
        diesel::delete(agent_tool_grants::table)
            .execute(&mut connection)
            .await
            .context("cannot revoke tool grants")
    }

    async fn delete(&self, digest: &[u8; 32]) {
        let deleted = async {
            let mut connection = self.database.get().await?;
            diesel::delete(agent_tool_grants::table.find(digest.as_slice()))
                .execute(&mut connection)
                .await?;
            anyhow::Ok(())
        };
        if let Err(error) = deleted.await {
            event!(
                name: "tools.grant.revoke_failure",
                Level::ERROR,
                error.message = %format!("{error:#}"),
                "a turn's tool grant could not be revoked; it is revoked when the engine next starts",
            );
        }
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

    /// Ends the grant: its turn's tools stop answering it.
    pub async fn revoke(mut self) {
        self.revoked = true;
        self.grants.delete(&self.digest).await;
    }
}

impl Drop for Grant {
    fn drop(&mut self) {
        if self.revoked {
            return;
        }
        let grants = self.grants.clone();
        let digest = self.digest;
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move { grants.delete(&digest).await });
            }
            Err(_no_runtime) => event!(
                name: "tools.grant.abandoned",
                Level::WARN,
                "a tool grant was dropped outside the runtime; it is revoked when the engine next starts",
            ),
        }
    }
}

fn digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
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

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_grant_works_until_it_is_revoked() {
        let (url, _connection) = crate::test_support::migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 2)
            .await
            .expect("Postgres accepts connections");
        let grants = Grants::new(database);
        let grant = grants
            .grant(&scope(), &[&work::SERVER])
            .await
            .expect("a grant is stored");
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

        let found = grants.find(&token).await.expect("readable").expect("live");
        assert_eq!(found.servers.len(), 1);
        let CallScope::Session(session) = found.scope else {
            panic!("the scope survives the round trip");
        };
        assert_eq!(session.session_id, 7);
        assert!(
            grants
                .find("not-a-token")
                .await
                .expect("readable")
                .is_none()
        );

        grant.revoke().await;
        assert!(
            grants.find(&token).await.expect("readable").is_none(),
            "revoking ends it"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_starting_engine_revokes_what_an_earlier_one_granted() {
        let (url, _connection) = crate::test_support::migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 2)
            .await
            .expect("Postgres accepts connections");
        let grants = Grants::new(database);
        let grant = grants
            .grant(&scope(), &[&work::SERVER])
            .await
            .expect("a grant is stored");
        let McpServer::Http { headers, .. } = &grant.mcp_servers()[0] else {
            panic!("the tools are served over HTTP");
        };
        let token = headers[0].1.expose_secret()["Bearer ".len()..].to_owned();

        assert_eq!(grants.revoke_all().await.expect("revoked"), 1);
        assert!(grants.find(&token).await.expect("readable").is_none());
        // The turn's own revoke afterwards finds nothing left to do.
        grant.revoke().await;
    }
}
