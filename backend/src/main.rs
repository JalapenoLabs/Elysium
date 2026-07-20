//! Elysium backend: a minimal Axum API wired to Postgres.
//!
//! The binary boots a Tokio runtime, initializes structured logging, lazily
//! creates a Postgres connection pool, and serves the routes defined in
//! [`api`]. It is intended as a starting point, not a finished service.

// mimalloc gives a meaningful, essentially free throughput win for services.
use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

mod accounts;
mod api;
mod audit;
mod authz;
mod bootstrap;
mod config;
mod context;
mod cookies;
mod credentials;
mod crypto;
mod enrollment;
mod events;
mod geoip;
mod grants;
mod invites;
mod login_events;
mod members;
mod oidc;
mod opaque;
mod orchestration;
mod orgs;
mod password;
mod policies;
mod profile;
mod projects;
mod proto;
mod protobuf;
mod rate_limit;
mod recovery;
mod security;
mod session;
mod sse;
mod state;
mod storage;
mod teams;
mod totp;
mod transport;
mod users;
mod webauthn;

use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

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
        .unwrap_or_else(|_| EnvFilter::new("info,elysium_backend=debug"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    // Load the secret-encryption key before touching the database, so a missing or
    // malformed key fails fast rather than after migrations run. Secrets at rest
    // (the OPAQUE setup, TOTP secrets) are sealed under it (see `crate::crypto`).
    let crypto = crypto::Crypto::from_env().wrap_err("failed to load the secret-encryption key")?;

    // Lazy pool: the process starts even if Postgres is briefly unavailable, and
    // reports real connectivity on demand.
    let pool = state::connect_pool()?;

    // Bring the database schema up to date before serving. This is the one place
    // startup genuinely needs Postgres; Compose gates the backend on a healthy
    // database, and the container restarts if it is not yet reachable.
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .wrap_err("failed to run database migrations")?;
    tracing::info!("database migrations applied");

    // The OPAQUE server setup lives in a table the migrations just created, so it
    // is loaded (or minted, on first boot) only now.
    let opaque_setup = Arc::new(
        opaque::load_or_create_setup(&pool, &crypto)
            .await
            .wrap_err("failed to initialize the OPAQUE server setup")?,
    );

    let mut state = AppState::new(pool, opaque_setup, crypto)
        .wrap_err("failed to initialize application state")?;

    // Attach the Redis cache handle now that we are in an async context. A failure is
    // non-fatal: cache-backed features (IP geolocation) fall back to their uncached
    // behavior rather than blocking startup.
    state.redis = state::connect_redis(state.redis_url.as_deref()).await;

    // Reconcile the "every project has a default Drive" invariant on boot. New projects
    // provision their default Drive inline; this backfills the projects created before
    // that wiring (migration 0022) so uploads and avatars work with zero admin config.
    storage::defaults::ensure_default_drives(&state.pool, &state.data_dir)
        .await
        .wrap_err("failed to reconcile default Drives")?;

    // Ensure an OIDC signing key exists so the provider can sign tokens from the
    // first request. Idempotent: it generates the initial RSA-2048 key only when
    // none is active, exactly like the OPAQUE setup mints itself on first boot.
    oidc::keys::ensure_active_key(&state.pool, &state.crypto)
        .await
        .wrap_err("failed to ensure an OIDC signing key")?;

    // Converge managed integrations to their desired state (docs/integrations.md):
    // a reboot or image upgrade never loses a provisioned service. Idempotent, so
    // re-running against an already-converged world is a no-op.
    {
        let mut conn = state
            .pool
            .acquire()
            .await
            .wrap_err("failed to acquire a connection for integration reconcile")?;
        orchestration::reconcile::reconcile_all(&mut conn, &state.provisioner, &state.crypto)
            .await
            .wrap_err("failed to reconcile managed integrations")?;
    }

    let port = env::var("API_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_API_PORT);
    let address = SocketAddr::from(([0, 0, 0, 0], port));

    let listener = tokio::net::TcpListener::bind(address)
        .await
        .wrap_err_with(|| format!("failed to bind {address}"))?;

    tracing::info!(address = %address, "Elysium API listening");

    // ConnectInfo exposes the socket peer address to handlers; the login history
    // uses it as the IP fallback when no proxy forwarded an address.
    axum::serve(
        listener,
        api::router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .wrap_err("server error")?;

    Ok(())
}
