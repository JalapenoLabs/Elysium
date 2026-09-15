// Copyright © 2026 Jalapeno Labs

//! Elysium OAuth broker: lets self-hosted Elysium instances connect Gmail and Outlook
//! accounts through OAuth apps someone else registered.
//!
//! Google and Microsoft only issue tokens to registered apps, and registering one
//! means a verified domain, a privacy policy, and for Gmail a security assessment.
//! The broker lets one community deployment hold those apps for every self-hosted
//! instance. It is stateless: it stores no tokens, no sessions, and no accounts. See
//! `README.md` for the protocol and for how to run your own.
//!
//! `elysium-oauth-broker generate-sealing-key` prints a key for `BROKER_SEALING_KEY`.

mod config;
mod pages;
mod pkce;
mod providers;
mod routes;
mod sealing;

use std::sync::Arc;

use anyhow::{Context, Result};
use mimalloc::MiMalloc;
use tokio::net::TcpListener;
use tracing::{Level, event};
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::routes::AppState;
use crate::sealing::Sealer;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::args().nth(1).as_deref() == Some("generate-sealing-key") {
        println!("{}", sealing::generate_key());
        return Ok(());
    }

    init_tracing();

    let config = Config::from_env().context("configuration is invalid")?;
    let sealer =
        Sealer::from_base64_key(&config.sealing_key).context("BROKER_SEALING_KEY is invalid")?;
    let http = reqwest::Client::builder()
        .user_agent(concat!("elysium-oauth-broker/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("cannot build the HTTP client")?;

    let bind_address = config.bind_address;
    let state = AppState {
        config: Arc::new(config),
        sealer: Arc::new(sealer),
        http,
    };

    let listener = TcpListener::bind(bind_address)
        .await
        .with_context(|| format!("cannot bind {bind_address}"))?;
    event!(name: "broker.listen.ready", Level::INFO, server.address = %bind_address, "listening");

    axum::serve(listener, routes::router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("http server failed")?;

    event!(name: "broker.shutdown.complete", Level::INFO, "shutdown complete");
    Ok(())
}

/// Structured logs to stdout. `RUST_LOG` filters; `LOG_FORMAT=json` emits one JSON
/// object per line.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let json = std::env::var("LOG_FORMAT").is_ok_and(|format| format.eq_ignore_ascii_case("json"));

    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false);
    if json {
        builder.json().init();
    } else {
        builder.compact().init();
    }
}

/// Resolves on SIGINT or SIGTERM, whichever arrives first.
async fn shutdown_signal() {
    let interrupt = async {
        tokio::signal::ctrl_c()
            .await
            .expect("the SIGINT handler installs");
    };
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("the SIGTERM handler installs")
            .recv()
            .await;
    };

    tokio::select! {
        () = interrupt => {},
        () = terminate => {},
    }
    event!(name: "broker.shutdown.begin", Level::INFO, "shutdown signal received");
}
