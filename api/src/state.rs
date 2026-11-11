// Copyright © 2026 Jalapeno Labs

//! Shared handler state.

use std::sync::Arc;

use redis::aio::ConnectionManager;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::action_items::links::Links;
use crate::agent_guidance::AgentGuidance;
use crate::auth::Auth;
use crate::backups::Backups;
use crate::browser::Browsers;
use crate::claude_accounts::ClaudeAccounts;
use crate::connectors::Connectors;
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::github::Github;
use crate::issue_reports::IssueReports;
use crate::jira::Jira;
use crate::knowledge::search::KnowledgeSearch;
use crate::mail::Mail;
use crate::meeting_uploads::staging::Staging;
use crate::meetings::sources::Sources;
use crate::middleware::rate_limit::Limiter;
use crate::passwords::remote::RemoteVault;
use crate::realtime::EventBus;
use crate::sessions::Sessions;
use crate::storage::Storage;
use crate::tools::grants::Grants;
use crate::updates::Updates;
use crate::version::VersionInfo;

/// Everything a request handler can reach. Cloning is cheap: each field is a handle.
///
/// The API runs no background work: whatever a request starts that outlives it, such as an
/// agent turn or a backup, it records for the engine to take (`crate::engine`).
#[derive(Clone)]
pub struct AppState {
    /// The guidance every connected agent reads as its session starts, kept for the next
    /// session. See `crate::agent_guidance`.
    pub agent_guidance: AgentGuidance,
    /// Kratos, and what the auth layers check requests against.
    pub auth: Auth,
    /// The backups of the databases, and Back up now. See `crate::backups`.
    pub backups: Backups,
    /// The browsers the host runs for people to sign in to their accounts in.
    pub browsers: Browsers,
    /// Claude accounts signed in through OAuth: their sign-ins and their usage. See
    /// `crate::claude_accounts`.
    pub claude_accounts: ClaudeAccounts,
    /// The connector hub's shared parts: the readers' clients, sync activity, and check runs.
    pub connectors: Connectors,
    pub database: Pool,
    pub redis: ConnectionManager,
    pub cipher: Arc<Cipher>,
    pub version: Arc<VersionInfo>,
    /// Publishes to every open `GET /api/v1/events` stream, and signals the engine.
    pub events: EventBus,
    pub github: Github,
    /// The live tool grants of every running turn, which `/api/agent-tools` checks.
    pub grants: Grants,
    /// Files problem reports on Elysium's own GitHub repository. See `crate::issue_reports`.
    pub issue_reports: IssueReports,
    pub jira: Jira,
    /// The knowledge base's search, by words and by meaning, with each person's budget for it.
    /// See `crate::knowledge::search`.
    pub knowledge: KnowledgeSearch,
    /// Action item links, through one provider trait, and the watcher's wake signal.
    pub links: Links,
    pub mail: Mail,
    /// Every meeting source's client, for a meeting connector's checks.
    pub meeting_sources: Sources,
    /// Where the parts of meetings people upload wait for their meeting's fetch. See
    /// `crate::meeting_uploads`.
    pub upload_staging: Staging,
    /// The password vault agents sign in to things with, which the engine runs. See
    /// `crate::passwords`.
    pub passwords: RemoteVault,
    /// Each person's budget for the global search, keyed by their user id. See `crate::search`.
    pub search_limiter: Arc<Limiter<Uuid>>,
    /// Coding and Studio sessions: their turns, events, and activity. See `crate::sessions`.
    pub sessions: Sessions,
    pub storage: Storage,
    /// Auto-update: what the updater is told, and the release history.
    pub updates: Updates,
    /// Cancelled when shutdown begins; long-lived responses end on it.
    pub shutdown: CancellationToken,
}
