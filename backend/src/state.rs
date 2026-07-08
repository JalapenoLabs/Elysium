//! Shared application state passed to every request handler.

use eyre::WrapErr;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::config::Deployment;
use crate::sse::SseRegistry;

/// Maximum number of pooled Postgres connections.
///
/// Small on purpose: the template runs a single API instance locally. Raise
/// this once the service scales horizontally or fans out concurrent queries.
const MAX_DB_CONNECTIONS: u32 = 5;

/// Handle to shared resources, cheaply cloneable into each handler.
///
/// [`PgPool`] is internally reference counted, [`SseRegistry`] holds its state
/// behind an `Arc`, and the WebAuthn RP is an `Arc`, so cloning `AppState` clones
/// handles rather than the underlying connections or caches.
#[derive(Clone, Debug)]
pub struct AppState {
    pub pool: PgPool,
    /// Per-user live SSE connection counts, for the connection cap.
    pub sse: SseRegistry,
    /// WebAuthn Relying Party: the API runs the passkey ceremony itself.
    pub webauthn: std::sync::Arc<webauthn_rs::Webauthn>,
}

impl AppState {
    /// Builds application state from the environment.
    ///
    /// The Postgres pool is lazy, so the API can start before Postgres is ready.
    ///
    /// # Errors
    /// Returns an error if `DATABASE_URL` is unset or the WebAuthn RP cannot be
    /// built from the derived config.
    pub fn from_env() -> eyre::Result<Self> {
        let database_url = std::env::var("DATABASE_URL").wrap_err("DATABASE_URL must be set")?;

        let pool = PgPoolOptions::new()
            .max_connections(MAX_DB_CONNECTIONS)
            .connect_lazy(&database_url)
            .wrap_err("failed to build Postgres connection pool")?;

        let deployment = Deployment::from_env();
        let webauthn = crate::webauthn::build(&deployment.rp_id, &deployment.app_origin)?;

        Ok(Self {
            pool,
            sse: SseRegistry::default(),
            webauthn,
        })
    }
}
