//! Shared application state passed to every request handler.

use std::sync::Arc;

use eyre::WrapErr;
use opaque_ke::ServerSetup;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::config::Deployment;
use crate::opaque::Suite;
use crate::sse::SseRegistry;

/// Maximum number of pooled Postgres connections.
///
/// Small on purpose: the template runs a single API instance locally. Raise
/// this once the service scales horizontally or fans out concurrent queries.
const MAX_DB_CONNECTIONS: u32 = 5;

/// Builds a lazy Postgres pool from `DATABASE_URL`.
///
/// The pool is lazy, so the API can start before Postgres is ready.
///
/// # Errors
/// Returns an error if `DATABASE_URL` is unset or the pool cannot be built.
pub fn connect_pool() -> eyre::Result<PgPool> {
    let database_url = std::env::var("DATABASE_URL").wrap_err("DATABASE_URL must be set")?;
    PgPoolOptions::new()
        .max_connections(MAX_DB_CONNECTIONS)
        .connect_lazy(&database_url)
        .wrap_err("failed to build Postgres connection pool")
}

/// Handle to shared resources, cheaply cloneable into each handler.
///
/// [`PgPool`] is internally reference counted, [`SseRegistry`] holds its state
/// behind an `Arc`, and the WebAuthn RP and OPAQUE setup are `Arc`s, so cloning
/// `AppState` clones handles rather than the underlying connections or state.
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    /// Per-user live SSE connection counts, for the connection cap.
    pub sse: SseRegistry,
    /// WebAuthn Relying Party: the API runs the passkey ceremony itself.
    pub webauthn: Arc<webauthn_rs::Webauthn>,
    /// The process-wide OPAQUE server setup for password auth.
    pub opaque_setup: Arc<ServerSetup<Suite>>,
    /// The app's own origin, used to build user-facing links (e.g. invites).
    pub app_origin: String,
}

impl AppState {
    /// Assembles application state around an already-built pool and OPAQUE setup.
    ///
    /// The pool and setup are passed in because the setup must be loaded from the
    /// database after migrations have run (see `main`), which the caller owns.
    ///
    /// # Errors
    /// Returns an error if the WebAuthn RP cannot be built from the derived config.
    pub fn new(pool: PgPool, opaque_setup: Arc<ServerSetup<Suite>>) -> eyre::Result<Self> {
        let deployment = Deployment::from_env();
        let webauthn = crate::webauthn::build(&deployment.rp_id, &deployment.app_origin)?;

        Ok(Self {
            pool,
            sse: SseRegistry::default(),
            webauthn,
            opaque_setup,
            app_origin: deployment.app_origin,
        })
    }
}
