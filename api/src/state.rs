// Copyright © 2026 Jalapeno Labs

//! Shared handler state.

use std::sync::Arc;

use redis::aio::ConnectionManager;
use tokio_util::sync::CancellationToken;

use crate::action_items::links::Links;
use crate::activity::Activities;
use crate::auth::Auth;
use crate::backups::Backups;
use crate::browser::Browsers;
use crate::claude_accounts::ClaudeAccounts;
use crate::connectors::Connectors;
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::elysia::Elysia;
use crate::github::Github;
use crate::jira::Jira;
use crate::mail::Mail;
use crate::meetings::Meetings;
use crate::passwords::Passwords;
use crate::realtime::EventBus;
use crate::runner::Runner;
use crate::sessions::Sessions;
use crate::storage::Storage;
use crate::updates::Updates;
use crate::version::VersionInfo;

/// Everything a request handler can reach. Cloning is cheap: each field is a handle.
#[derive(Clone)]
pub struct AppState {
    /// Kratos, and what the auth layers check requests against.
    pub auth: Auth,
    /// The nightly backups of the databases, and Back up now. See `crate::backups`.
    pub backups: Backups,
    /// The browsers the host runs for people to sign in to their accounts in.
    pub browsers: Browsers,
    /// Claude accounts signed in through OAuth: their sign-ins, the keeper that refreshes them,
    /// and their usage. See `crate::claude_accounts`.
    pub claude_accounts: ClaudeAccounts,
    /// The connector hub's shared parts: the mailbox reader, sync activity, and check runs.
    pub connectors: Connectors,
    pub database: Pool,
    /// Work a restart would cut short, such as agent turns. Auto-update waits for it.
    #[expect(
        dead_code,
        reason = "no handler begins an activity yet; the runner and auto-update hold their own handles"
    )]
    pub activity: Activities,
    pub redis: ConnectionManager,
    pub cipher: Arc<Cipher>,
    pub version: Arc<VersionInfo>,
    /// Publishes to every open `GET /api/v1/events` stream.
    pub events: EventBus,
    /// Elysia, and the signal that tells her dispatcher to look at the queue.
    pub elysia: Elysia,
    pub github: Github,
    pub jira: Jira,
    /// Action item links, through one provider trait, and the watcher's wake signal.
    pub links: Links,
    pub mail: Mail,
    /// Meetings read from meeting sources and cut into timeline segments. See
    /// `crate::meetings`.
    pub meetings: Meetings,
    /// The password vault agents sign in to things with. See `crate::passwords`.
    pub passwords: Passwords,
    /// Runs agent turns in the workspace container. See `crate::runner`.
    pub runner: Runner,
    /// Coding and Studio sessions' turns, events, and activity. See `crate::sessions`.
    pub sessions: Sessions,
    pub storage: Storage,
    /// Auto-update: what the updater is told, and the release history.
    pub updates: Updates,
    /// Cancelled when shutdown begins; long-lived responses end on it.
    pub shutdown: CancellationToken,
}
