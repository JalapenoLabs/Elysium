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
//! and only its SHA-256 is stored, so the table never holds a usable credential.
//!
//! A grant is leased. While its [`Grant`] is held, a task of its own renews it every
//! `RENEW_INTERVAL` for another `LEASE_SECONDS`; a grant whose lease lapsed is refused, and
//! the next grant minted sweeps it away. The engine revokes a grant when its turn ends
//! ([`Grant::revoke`]), and one left by an engine that died lapses on its own within a lease. So
//! nothing revokes grants when an engine starts, and a grant one loop has just minted is never
//! taken from it by another loop, or by another engine. Lease times are Postgres's clock alone,
//! so the engine that writes them and the API that reads them never disagree.

use std::time::Duration;

use anyhow::Context as _;
use diesel::dsl::{IntervalDsl as _, now};
use diesel::prelude::*;
use diesel::sql_types::Timestamptz;
use diesel_async::RunQueryDsl;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest as _, Sha256};
use tokio::task::JoinHandle;
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

/// How long a grant lasts past its last renewal, in seconds. Several renewals long, so a slow
/// database never lets a running turn's grant lapse; short enough that a grant left by an engine
/// that died is refused within a minute.
const LEASE_SECONDS: i32 = 60;

/// How often a held grant is renewed: four times per lease, as workflow steps' leases are.
const RENEW_INTERVAL: Duration = Duration::from_secs(15);

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

/// A grant held for one turn, renewed for as long as it is held. [`Grant::revoke`] ends it;
/// dropping it unrevoked, such as when a turn's follower panics, revokes it in the background.
#[derive(Debug)]
pub struct Grant {
    grants: Grants,
    digest: [u8; 32],
    token: SecretString,
    servers: Vec<&'static ToolServer>,
    /// The task renewing the lease; `None` once the grant is revoked.
    renewal: Option<JoinHandle<()>>,
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

    /// Mints a grant for `scope` to call `servers`, made by the agent the scope names, and
    /// sweeps away every grant whose lease lapsed.
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
            // A pool worker is a coding agent too, working a batch rather than a session.
            CallScope::Session(_) | CallScope::Worker(_) => CODING_AGENT_USER_ID,
            // Segmenting takes what it reads as Elysia's.
            CallScope::Elysia(_) | CallScope::Segmenting(_) => ELYSIA_USER_ID,
            CallScope::ProjectManager(_) => PROJECT_MANAGER_USER_ID,
        };

        let mut connection = self
            .database
            .get()
            .await
            .context("no database connection to store a tool grant")?;
        // Lapsed grants are refused already; sweeping keeps the table to what turns hold.
        diesel::delete(agent_tool_grants::table.filter(agent_tool_grants::expires_at.le(now)))
            .execute(&mut connection)
            .await
            .context("cannot sweep lapsed tool grants")?;
        diesel::insert_into(agent_tool_grants::table)
            .values((
                NewGrant {
                    token_sha256: &digest,
                    scope: serde_json::to_value(scope).expect("a call scope serializes"),
                    servers: servers.iter().map(|server| server.name).collect(),
                    created_by,
                },
                agent_tool_grants::expires_at
                    .eq(now.into_sql::<Timestamptz>() + LEASE_SECONDS.seconds()),
            ))
            .execute(&mut connection)
            .await
            .context("cannot store a tool grant")?;
        Ok(Grant {
            grants: self.clone(),
            digest,
            token: SecretString::from(token),
            servers: servers.to_vec(),
            renewal: Some(tokio::spawn(self.clone().keep_renewed(digest))),
        })
    }

    /// What `token` grants, if it is live: stored, and its lease not lapsed. A stored server
    /// this build does not serve is left out, and a scope it cannot read refuses the grant.
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
            .filter(agent_tool_grants::expires_at.gt(now))
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

    /// Renews the grant `digest` every [`RENEW_INTERVAL`] until its [`Grant`] stops this task.
    /// A renewal that fails is tried again at the next interval, since the lease outlasts
    /// several; a grant found lapsed has nothing left to renew.
    async fn keep_renewed(self, digest: [u8; 32]) {
        // The grant was stored with a full lease, so the first renewal is one interval away.
        let first = tokio::time::Instant::now() + RENEW_INTERVAL;
        let mut renewals = tokio::time::interval_at(first, RENEW_INTERVAL);
        renewals.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            renewals.tick().await;
            match self.renew(&digest).await {
                Ok(true) => {}
                Ok(false) => {
                    event!(
                        name: "tools.grant.lapsed",
                        Level::WARN,
                        "a turn's tool grant lapsed before it was renewed; its tools refuse it now",
                    );
                    return;
                }
                Err(error) => event!(
                    name: "tools.grant.renew_failure",
                    Level::WARN,
                    error.message = %format!("{error:#}"),
                    "could not renew a turn's tool grant; trying again before it lapses",
                ),
            }
        }
    }

    /// Extends the live grant `digest` by a full lease. False when it lapsed or is gone.
    async fn renew(&self, digest: &[u8; 32]) -> anyhow::Result<bool> {
        let mut connection = self
            .database
            .get()
            .await
            .context("no database connection to renew a tool grant")?;
        let renewed = diesel::update(
            agent_tool_grants::table
                .find(digest.as_slice())
                .filter(agent_tool_grants::expires_at.gt(now)),
        )
        .set(
            agent_tool_grants::expires_at
                .eq(now.into_sql::<Timestamptz>() + LEASE_SECONDS.seconds()),
        )
        .execute(&mut connection)
        .await
        .context("cannot renew a tool grant")?;
        Ok(renewed == 1)
    }

    /// Stops `renewal`, then deletes the grant `digest`. The renewal is awaited first, so none
    /// can run after the delete and report the grant as lapsed.
    async fn end(&self, renewal: JoinHandle<()>, digest: &[u8; 32]) {
        renewal.abort();
        // Cancelled is how it is meant to end, and a renewal has nothing else to report.
        let _ended = renewal.await;
        self.delete(digest).await;
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
                "a turn's tool grant could not be revoked; it lapses within a minute",
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
        if let Some(renewal) = self.renewal.take() {
            self.grants.end(renewal, &self.digest).await;
        }
    }
}

impl Drop for Grant {
    fn drop(&mut self) {
        let Some(renewal) = self.renewal.take() else {
            // Revoked already.
            return;
        };
        // Stopped here, so a grant dropped outside the runtime is no longer renewed either.
        renewal.abort();
        let grants = self.grants.clone();
        let digest = self.digest;
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move { grants.end(renewal, &digest).await });
            }
            Err(_no_runtime) => event!(
                name: "tools.grant.abandoned",
                Level::WARN,
                "a tool grant was dropped outside the runtime; it lapses within a minute",
            ),
        }
    }
}

fn digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeDelta, Utc};

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

    async fn grants() -> Grants {
        let (url, _connection) = crate::test_support::migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 2)
            .await
            .expect("Postgres accepts connections");
        Grants::new(database)
    }

    fn token(grant: &Grant) -> String {
        let McpServer::Http { headers, .. } = &grant.mcp_servers()[0] else {
            panic!("the tools are served over HTTP");
        };
        headers[0]
            .1
            .expose_secret()
            .strip_prefix("Bearer ")
            .expect("a bearer")
            .to_owned()
    }

    /// Moves grant `digest`'s lease to end at `expires_at`, as time passing would.
    async fn lease_ends_at(grants: &Grants, digest: &[u8; 32], expires_at: DateTime<Utc>) {
        let mut connection = grants.database.get().await.expect("a connection");
        diesel::update(agent_tool_grants::table.find(digest.as_slice()))
            .set(agent_tool_grants::expires_at.eq(expires_at))
            .execute(&mut connection)
            .await
            .expect("the lease moves");
    }

    async fn stored(grants: &Grants, digest: &[u8; 32]) -> Option<DateTime<Utc>> {
        let mut connection = grants.database.get().await.expect("a connection");
        agent_tool_grants::table
            .find(digest.as_slice())
            .select(agent_tool_grants::expires_at)
            .first(&mut connection)
            .await
            .optional()
            .expect("readable")
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_grant_works_until_it_is_revoked() {
        let grants = grants().await;
        let grant = grants
            .grant(&scope(), &[&work::SERVER])
            .await
            .expect("a grant is stored");
        let McpServer::Http { url, .. } = &grant.mcp_servers()[0] else {
            panic!("the tools are served over HTTP");
        };
        assert_eq!(url.as_str(), "http://api:8080/api/agent-tools/elysium_work");
        let token = token(&grant);

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

        let digest = grant.digest;
        grant.revoke().await;
        assert!(
            grants.find(&token).await.expect("readable").is_none(),
            "revoking ends it"
        );
        assert!(stored(&grants, &digest).await.is_none(), "and removes it");
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_lapsed_grant_is_refused_and_the_next_grant_sweeps_it_away() {
        let grants = grants().await;
        // As an engine that died leaves it: stored, and no longer renewed.
        let abandoned = grants
            .grant(&scope(), &[&work::SERVER])
            .await
            .expect("a grant is stored");
        let abandoned_digest = abandoned.digest;
        let abandoned_token = token(&abandoned);
        lease_ends_at(
            &grants,
            &abandoned_digest,
            Utc::now() - TimeDelta::seconds(1),
        )
        .await;

        assert!(
            grants
                .find(&abandoned_token)
                .await
                .expect("readable")
                .is_none(),
            "a lapsed grant is refused"
        );
        assert!(
            !grants.renew(&abandoned_digest).await.expect("writable"),
            "and is not brought back by a late renewal"
        );

        let next = grants
            .grant(&scope(), &[&work::SERVER])
            .await
            .expect("a grant is stored");
        assert!(
            stored(&grants, &abandoned_digest).await.is_none(),
            "the next grant sweeps the lapsed one away"
        );
        assert!(
            grants
                .find(&token(&next))
                .await
                .expect("readable")
                .is_some(),
            "and is live itself"
        );
        abandoned.revoke().await;
        next.revoke().await;
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn renewing_a_held_grant_extends_it_by_a_full_lease() {
        let grants = grants().await;
        let grant = grants
            .grant(&scope(), &[&work::SERVER])
            .await
            .expect("a grant is stored");
        let granted_until = stored(&grants, &grant.digest).await.expect("stored");
        assert!(
            granted_until > Utc::now() + TimeDelta::seconds(50),
            "a full lease to start"
        );

        lease_ends_at(&grants, &grant.digest, Utc::now() + TimeDelta::seconds(5)).await;
        assert!(grants.renew(&grant.digest).await.expect("writable"));
        let renewed_until = stored(&grants, &grant.digest).await.expect("stored");
        assert!(
            renewed_until > Utc::now() + TimeDelta::seconds(50),
            "a renewal extends it by a full lease"
        );
        grant.revoke().await;
    }
}
