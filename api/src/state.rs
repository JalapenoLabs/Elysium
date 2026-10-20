// Copyright © 2026 Jalapeno Labs

//! Shared handler state.

use std::sync::Arc;

use redis::aio::ConnectionManager;
use tokio_util::sync::CancellationToken;

use crate::action_items::links::Links;
use crate::activity::Activities;
use crate::auth::Auth;
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::fleet::Fleet;
use crate::github::Github;
use crate::jira::Jira;
use crate::mail::Mail;
use crate::realtime::EventBus;
use crate::storage::Storage;
use crate::updates::Updates;
use crate::version::VersionInfo;

/// Everything a request handler can reach. Cloning is cheap: each field is a handle.
#[derive(Clone)]
pub struct AppState {
    /// Kratos, and what the auth layers check requests against.
    pub auth: Auth,
    pub database: Pool,
    /// Work a restart would cut short, such as agent turns. Auto-update waits for it.
    #[allow(
        dead_code,
        reason = "the host runner begins activities here; remove once it does"
    )]
    pub activity: Activities,
    pub redis: ConnectionManager,
    pub cipher: Arc<Cipher>,
    pub version: Arc<VersionInfo>,
    /// Publishes to every open `GET /api/v1/events` stream.
    pub events: EventBus,
    pub fleet: Fleet,
    pub github: Github,
    pub jira: Jira,
    /// Action item links, through one provider trait, and the watcher's wake signal.
    pub links: Links,
    pub mail: Mail,
    pub storage: Storage,
    /// Auto-update: what the updater is told, and the release history.
    pub updates: Updates,
    /// Cancelled when shutdown begins; long-lived responses end on it.
    pub shutdown: CancellationToken,
}
