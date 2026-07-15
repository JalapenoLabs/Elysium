//! Shared application state passed to every request handler.

use std::sync::Arc;

use eyre::WrapErr;
use opaque_ke::ServerSetup;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::config::Deployment;
use crate::crypto::Crypto;
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
    /// Seals and opens secrets at rest (OPAQUE setup, TOTP secrets).
    pub crypto: Crypto,
    /// Redis connection string when configured. `/health` reports its presence.
    pub redis_url: Option<String>,
    /// Reconnecting Redis handle when a connection was established (see
    /// [`connect_redis`]). `None` when Redis is unconfigured or unreachable, in which
    /// case cache-backed features degrade gracefully rather than fail.
    pub redis: Option<redis::aio::ConnectionManager>,
    /// Shared outbound HTTP client (connection-pooled, cheap to clone), used for
    /// best-effort IP geolocation.
    pub http: reqwest::Client,
    /// Geolocation provider endpoint when the feature is enabled (see
    /// `crate::geoip`); `None` disables IP geolocation.
    pub geoip_endpoint: Option<String>,
    /// RabbitMQ connection string when configured. Same wiring-only status as
    /// `redis_url`.
    pub rabbitmq_url: Option<String>,
    /// The app's own origin, used to build user-facing links (e.g. invites).
    pub app_origin: String,
}

impl AppState {
    /// Assembles application state around an already-built pool and OPAQUE setup.
    ///
    /// The pool and setup are passed in because the setup must be loaded from the
    /// database after migrations have run (see `main`), which the caller owns.
    /// Redis and RabbitMQ are optional: an unset or blank variable reads as absent.
    ///
    /// # Errors
    /// Returns an error if the WebAuthn RP cannot be built from the derived config.
    pub fn new(
        pool: PgPool,
        opaque_setup: Arc<ServerSetup<Suite>>,
        crypto: Crypto,
    ) -> eyre::Result<Self> {
        let deployment = Deployment::from_env();
        let webauthn = crate::webauthn::build(&deployment.rp_id, &deployment.app_origin)?;

        Ok(Self {
            pool,
            sse: SseRegistry::default(),
            webauthn,
            opaque_setup,
            crypto,
            redis_url: read_optional("REDIS_URL"),
            // The reconnecting handle is attached by `main` after boot, because
            // establishing it is async; it starts absent.
            redis: None,
            http: reqwest::Client::new(),
            geoip_endpoint: crate::geoip::endpoint_from_env(),
            rabbitmq_url: read_optional("RABBITMQ_URL"),
            app_origin: deployment.app_origin,
        })
    }
}

/// How long to wait for the initial Redis connection before giving up.
///
/// Compose gates the API on a healthy Redis, so this only guards the rare case where
/// Redis is briefly unreachable at boot; the API still starts, with caches disabled.
const REDIS_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Builds a reconnecting Redis handle from an optional connection string.
///
/// Returns `None` when the URL is absent or blank, cannot be parsed, or the initial
/// connection does not complete within [`REDIS_CONNECT_TIMEOUT`]. A `None` result is not
/// fatal: cache-backed features fall back to their uncached behavior. Once connected,
/// the manager reconnects on its own after transient drops.
pub async fn connect_redis(url: Option<&str>) -> Option<redis::aio::ConnectionManager> {
    let url = url.filter(|value| !value.trim().is_empty())?;

    let client = match redis::Client::open(url) {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(error = %error, "invalid REDIS_URL; caches disabled");
            return None;
        }
    };

    match tokio::time::timeout(
        REDIS_CONNECT_TIMEOUT,
        redis::aio::ConnectionManager::new(client),
    )
    .await
    {
        Ok(Ok(manager)) => {
            tracing::info!("redis cache connected");
            Some(manager)
        }
        Ok(Err(error)) => {
            tracing::warn!(error = %error, "redis connection failed; caches disabled");
            None
        }
        Err(_) => {
            tracing::warn!("redis connection timed out; caches disabled");
            None
        }
    }
}

/// Reads an environment variable, treating unset or blank values as absent.
fn read_optional(key: &str) -> Option<String> {
    match std::env::var(key) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => None,
    }
}
