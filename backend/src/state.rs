//! Shared application state passed to every request handler.

use eyre::WrapErr;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::auth::Auth;
use crate::config::Deployment;
use crate::oidc::OidcConfig;
use crate::sse::SseRegistry;

/// Maximum number of pooled Postgres connections.
///
/// Small on purpose: the template runs a single API instance locally. Raise
/// this once the service scales horizontally or fans out concurrent queries.
const MAX_DB_CONNECTIONS: u32 = 5;

/// Handle to shared resources, cheaply cloneable into each handler.
///
/// [`PgPool`] is internally reference counted, [`Auth`] and [`reqwest::Client`]
/// hold their state behind an `Arc`, and [`OidcConfig`] is small, so cloning
/// `AppState` clones handles rather than the underlying connections or caches.
#[derive(Clone, Debug)]
pub struct AppState {
    pub pool: PgPool,
    pub auth: Auth,
    pub oidc: OidcConfig,
    /// Shared client for backchannel calls to Keycloak (token exchange, logout).
    pub http: reqwest::Client,
    /// Per-user live SSE connection counts, for the connection cap.
    pub sse: SseRegistry,
}

impl AppState {
    /// Builds application state from the environment.
    ///
    /// The Postgres pool is lazy, so the API can start before Postgres is ready.
    /// Token validation ([`Auth`]) likewise fetches its keys on demand, so the
    /// API can also start before Keycloak is reachable.
    ///
    /// # Errors
    /// Returns an error if `DATABASE_URL` or `OIDC_CLIENT_SECRET` is unset, or a
    /// client cannot be built. Every URL is derived (see [`Deployment`]).
    pub fn from_env() -> eyre::Result<Self> {
        let database_url = std::env::var("DATABASE_URL").wrap_err("DATABASE_URL must be set")?;

        let pool = PgPoolOptions::new()
            .max_connections(MAX_DB_CONNECTIONS)
            .connect_lazy(&database_url)
            .wrap_err("failed to build Postgres connection pool")?;

        let deployment = Deployment::from_env();
        let auth = Auth::new(deployment.issuer.clone(), deployment.jwks_url.clone())?;
        let client_secret =
            std::env::var("OIDC_CLIENT_SECRET").wrap_err("OIDC_CLIENT_SECRET must be set")?;
        let oidc = OidcConfig::new(&deployment, client_secret);
        let http = reqwest::Client::builder()
            .build()
            .wrap_err("failed to build the backchannel HTTP client")?;

        Ok(Self {
            pool,
            auth,
            oidc,
            http,
            sse: SseRegistry::default(),
        })
    }
}
