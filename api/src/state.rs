// Copyright © 2026 Jalapeno Labs

//! Shared handler state.

use std::sync::Arc;

use redis::aio::ConnectionManager;
use tokio_util::sync::CancellationToken;

use crate::crypto::Cipher;
use crate::database::Pool;
use crate::fleet::Fleet;
use crate::mail::Mail;
use crate::realtime::EventBus;
use crate::storage::Storage;
use crate::version::VersionInfo;

/// Everything a request handler can reach. Cloning is cheap: each field is a handle.
#[derive(Clone)]
pub struct AppState {
    pub database: Pool,
    pub redis: ConnectionManager,
    pub cipher: Arc<Cipher>,
    pub version: Arc<VersionInfo>,
    /// Publishes to every open `GET /api/v1/events` stream.
    pub events: EventBus,
    pub fleet: Fleet,
    pub mail: Mail,
    pub storage: Storage,
    /// Cancelled when shutdown begins; long-lived responses end on it.
    pub shutdown: CancellationToken,
}
