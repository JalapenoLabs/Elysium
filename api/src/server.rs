// Copyright © 2026 Jalapeno Labs

//! The HTTP server: startup, middleware stack, and graceful shutdown.
//!
//! Startup order: configuration, encryption key, schema check, store connections
//! (with retry), router, listener. Shutdown order is the reverse: stop accepting,
//! drain in-flight requests, close the Postgres pool, drop the Redis connection.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::timeout::TimeoutLayer;
use tracing::{Level, event};

use crate::config::Config;
use crate::crypto::Cipher;
use crate::database::migrations;
use crate::state::AppState;
use crate::version::VersionInfo;
use crate::{connections, middleware, routes, shutdown};

/// Serves the API until a shutdown signal arrives.
///
/// # Errors
/// Fails on invalid configuration, an invalid encryption key, pending migrations,
/// unreachable stores, or a listener that cannot bind.
pub async fn serve() -> Result<()> {
    let config = Config::from_env().context("configuration is invalid")?;
    let version = Arc::new(VersionInfo::from_build()?);

    event!(
        name: "api.startup.begin",
        Level::INFO,
        service.name = version.name,
        service.version = version.version,
        vcs.ref.head.revision = version.git.commit.as_ref().map_or("", |commit| commit.short_hash.as_str()),
        "starting",
    );

    let cipher = Arc::new(
        Cipher::from_base64_key(&config.encryption_key)
            .context("ELYSIUM_ENCRYPTION_KEY is invalid")?,
    );

    let database =
        connections::connect_postgres(&config.database_url, config.database_max_connections)
            .await?;
    migrations::ensure_up_to_date(config.database_url.clone()).await?;
    let redis = connections::connect_redis(&config.redis_url).await?;

    let state = AppState {
        database: database.clone(),
        redis: redis.clone(),
        cipher,
        version,
    };
    let rate_limiter = middleware::rate_limit::build();
    let app = build_router(&config, rate_limiter.layer).with_state(state);

    let listener = TcpListener::bind(config.bind_address)
        .await
        .with_context(|| format!("cannot bind {}", config.bind_address))?;
    event!(
        name: "api.listen.ready",
        Level::INFO,
        server.address = %config.bind_address,
        "listening",
    );

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown::signal())
    .await
    .context("http server failed")?;

    // serve() returning means every in-flight request has finished and the router,
    // with its copies of the handles, is gone. Closing the pool drops its idle
    // connections; the Redis manager is a multiplexed socket that closes when its
    // last handle drops, which is this one.
    rate_limiter.sweeper.abort();
    database.close();
    event!(name: "connection.close.success", Level::INFO, db.system.name = "postgres", "store connection closed");
    drop(redis);
    event!(name: "connection.close.success", Level::INFO, db.system.name = "redis", "store connection closed");
    event!(name: "api.shutdown.complete", Level::INFO, "shutdown complete");

    Ok(())
}

/// Assembles middleware around the routes. Outermost layers run first on the way
/// in and last on the way out, so request ids exist before anything logs, and the
/// security headers cover every response including rate limit rejections.
fn build_router(config: &Config, rate_limit: middleware::rate_limit::Layer) -> Router<AppState> {
    let layers = ServiceBuilder::new()
        .layer(middleware::trace::set_request_id_layer())
        .layer(middleware::trace::propagate_request_id_layer())
        .layer(middleware::trace::layer())
        .layer(CatchPanicLayer::new())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            config.request_timeout,
        ))
        .layer(DefaultBodyLimit::max(config.max_request_body_bytes))
        .layer(axum::middleware::from_fn(
            middleware::security_headers::apply,
        ))
        .layer(middleware::cors::layer(config.cors_allowed_origins.clone()))
        .layer(rate_limit);

    routes::router().layer(layers)
}
