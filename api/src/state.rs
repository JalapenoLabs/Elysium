// Copyright © 2026 Jalapeno Labs

//! Shared handler state.

use std::sync::Arc;

use redis::aio::ConnectionManager;

use crate::crypto::Cipher;
use crate::database::Pool;
use crate::version::VersionInfo;

/// Everything a request handler can reach. Cloning is cheap: each field is a handle.
#[derive(Clone)]
pub struct AppState {
    pub database: Pool,
    #[expect(
        dead_code,
        reason = "no route touches Redis yet; handlers reach it through this handle"
    )]
    pub redis: ConnectionManager,
    pub cipher: Arc<Cipher>,
    pub version: Arc<VersionInfo>,
}
