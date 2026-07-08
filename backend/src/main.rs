//! Elysia backend: a minimal Axum API wired to Postgres.
//!
//! The binary boots a Tokio runtime, initializes structured logging, lazily
//! creates a Postgres connection pool, and serves the routes defined in
//! [`api`]. It is intended as a starting point, not a finished service.

// mimalloc gives a meaningful, essentially free throughput win for services.
use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

mod api;
mod auth;
mod config;
mod oidc;
mod proto;
mod protobuf;
mod session;
mod sse;
mod state;

use std::env;
use std::net::SocketAddr;

use eyre::WrapErr;
use tracing_subscriber::EnvFilter;

use crate::state::AppState;

/// Port the API listens on when `API_PORT` is unset.
///
/// Matches the value published by Compose. Digits of pi (31416) so the
/// template does not collide with the usual 3000/8080 crowd during local dev.
const DEFAULT_API_PORT: u16 = 31416;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    // Respect `RUST_LOG` when present, otherwise default to a sensible level.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,elysia_backend=debug"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    // Lazy pool and lazy JWKS: the process starts even if Postgres or Keycloak
    // are briefly unavailable, and reports real connectivity on demand.
    let state = AppState::from_env().wrap_err("failed to initialize application state")?;

    // Bring the database schema up to date before serving. This is the one place
    // startup genuinely needs Postgres; Compose gates the backend on a healthy
    // database, and the container restarts if it is not yet reachable.
    sqlx::migrate!("./migrations")
        .run(&state.pool)
        .await
        .wrap_err("failed to run database migrations")?;
    tracing::info!("database migrations applied");

    let port = env::var("API_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_API_PORT);
    let address = SocketAddr::from(([0, 0, 0, 0], port));

    let listener = tokio::net::TcpListener::bind(address)
        .await
        .wrap_err_with(|| format!("failed to bind {address}"))?;

    tracing::info!(address = %address, "Elysia API listening");

    axum::serve(listener, api::router(state))
        .await
        .wrap_err("server error")?;

    Ok(())
}
