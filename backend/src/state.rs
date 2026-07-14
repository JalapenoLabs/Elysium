//! Shared application state passed to every request handler.

use eyre::WrapErr;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::auth::Auth;

/// Maximum number of pooled Postgres connections.
///
/// Small on purpose: the template runs a single API instance locally. Raise
/// this once the service scales horizontally or fans out concurrent queries.
const MAX_DB_CONNECTIONS: u32 = 5;

/// Handle to shared resources, cheaply cloneable into each handler.
///
/// [`PgPool`] is internally reference counted and [`Auth`] holds its state behind
/// an `Arc`, so cloning `AppState` clones handles rather than the underlying
/// connections or key cache.
#[derive(Clone, Debug)]
pub struct AppState {
    pub pool: PgPool,
    pub auth: Auth,
    /// Redis connection string when configured. Wiring only for now: `/health`
    /// reports its presence, and a real client lands with the first feature
    /// that needs the cache.
    pub redis_url: Option<String>,
    /// RabbitMQ connection string when configured. Same wiring-only status as
    /// `redis_url`.
    pub rabbitmq_url: Option<String>,
}

impl AppState {
    /// Builds application state from the environment.
    ///
    /// The Postgres pool is lazy, so the API can start before Postgres is ready.
    /// Token validation ([`Auth`]) likewise fetches its keys on demand, so the
    /// API can also start before Keycloak is reachable. Redis and RabbitMQ are
    /// optional: unset or blank variables simply mean "not configured".
    ///
    /// # Errors
    /// Returns an error if `DATABASE_URL` or the Keycloak variables are unset or
    /// cannot be parsed.
    pub fn from_env() -> eyre::Result<Self> {
        let database_url = std::env::var("DATABASE_URL").wrap_err("DATABASE_URL must be set")?;

        let pool = PgPoolOptions::new()
            .max_connections(MAX_DB_CONNECTIONS)
            .connect_lazy(&database_url)
            .wrap_err("failed to build Postgres connection pool")?;

        let auth = Auth::from_env()?;

        Ok(Self {
            pool,
            auth,
            redis_url: read_optional("REDIS_URL"),
            rabbitmq_url: read_optional("RABBITMQ_URL"),
        })
    }
}

/// Reads an environment variable, treating unset or blank values as absent.
fn read_optional(key: &str) -> Option<String> {
    match std::env::var(key) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => None,
    }
}
