//! Shared application state passed to every request handler.

use eyre::WrapErr;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Maximum number of pooled Postgres connections.
///
/// Small on purpose: the template runs a single API instance locally. Raise
/// this once the service scales horizontally or fans out concurrent queries.
const MAX_DB_CONNECTIONS: u32 = 5;

/// Handle to shared resources, cheaply cloneable into each handler.
///
/// [`PgPool`] is internally reference counted, so cloning `AppState` clones a
/// pool handle rather than the underlying connections.
#[derive(Clone, Debug)]
pub struct AppState {
    pub pool: PgPool,
}

impl AppState {
    /// Builds application state with a lazily connected Postgres pool.
    ///
    /// The pool does not open a connection until the first query, so the API
    /// can start before Postgres is ready and recover without a restart.
    ///
    /// # Errors
    /// Returns an error if `DATABASE_URL` is unset or cannot be parsed.
    pub fn connect_lazy() -> eyre::Result<Self> {
        let database_url = std::env::var("DATABASE_URL").wrap_err("DATABASE_URL must be set")?;

        let pool = PgPoolOptions::new()
            .max_connections(MAX_DB_CONNECTIONS)
            .connect_lazy(&database_url)
            .wrap_err("failed to build Postgres connection pool")?;

        Ok(Self { pool })
    }
}
