// Copyright © 2026 Jalapeno Labs

//! The engine's one HTTP route: `GET /health`, for its compose health check.
//!
//! The engine serves nothing else. It answers once startup reached it, so a healthy engine is
//! one whose stores connected and whose schema is current; compose's `--wait`, and so every
//! deploy and update, waits for it.

use std::net::SocketAddr;

use anyhow::{Context, Result};
use axum::Router;
use axum::routing::get;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

/// The path compose's health check asks.
pub const HEALTH_PATH: &str = "/health";

/// Binds `address` and answers the health check until `shutdown`.
///
/// # Errors
/// Fails when the address cannot be bound.
pub async fn serve(address: SocketAddr, shutdown: CancellationToken) -> Result<JoinHandle<()>> {
    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("cannot bind {address} for the health check"))?;
    let router = Router::new().route(HEALTH_PATH, get(|| async { "ok" }));
    event!(
        name: "engine.health.ready",
        Level::INFO,
        server.address = %address,
        "answering health checks",
    );
    Ok(tokio::spawn(async move {
        let served = axum::serve(listener, router)
            .with_graceful_shutdown(async move { shutdown.cancelled().await })
            .await;
        if let Err(error) = served {
            event!(
                name: "engine.health.failure",
                Level::ERROR,
                error.message = %error,
                "the health check stopped answering",
            );
        }
    }))
}
