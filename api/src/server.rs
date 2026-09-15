// Copyright © 2026 Jalapeno Labs

//! The HTTP server: startup, middleware stack, and graceful shutdown.
//!
//! Startup order: configuration, encryption key, schema check, store connections
//! (with retry), fleet watchers, router, listener. Shutdown order is the reverse: a
//! signal cancels the shutdown token, which ends every event stream and fleet
//! watcher; the server stops accepting and drains in-flight requests; the watchers
//! are awaited; then the Postgres pool closes and the Redis connection drops.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use bollard::{API_DEFAULT_VERSION, Docker};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower::ServiceBuilder;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::timeout::TimeoutLayer;
use tracing::{Level, event};

use crate::config::Config;
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::database::migrations;
use crate::fleet::Fleet;
use crate::mail::Mail;
use crate::mail::broker::Broker;
use crate::mail::dns::DnsChecker;
use crate::mail::hosting::Hosting;
use crate::mail::stalwart::Stalwart;
use crate::mail::stalwart::container::StalwartContainer;
use crate::realtime::EventBus;
use crate::state::AppState;
use crate::version::VersionInfo;
use crate::{connections, middleware, routes, shutdown};

/// How long one Docker call may take. Pulling the mail server's image is one call, and
/// can take minutes on a slow connection.
const DOCKER_TIMEOUT_SECONDS: u64 = 600;

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

    let shutdown = CancellationToken::new();
    let events = EventBus::new();
    let fleet = Fleet::new(
        database.clone(),
        Arc::clone(&cipher),
        events.clone(),
        shutdown.clone(),
    );
    fleet.start().await?;
    let mail = build_mail(
        &config,
        database.clone(),
        Arc::clone(&cipher),
        events.clone(),
    )?;
    // Brings an existing mail server back up; pulling its image may take a while, so
    // the API does not wait for it.
    let hosting = mail.hosting.clone();
    tokio::spawn(async move { hosting.reconcile().await });

    let state = AppState {
        database: database.clone(),
        redis: redis.clone(),
        cipher,
        version,
        events,
        fleet: fleet.clone(),
        mail,
        shutdown: shutdown.clone(),
    };
    let rate_limiter = middleware::rate_limit::build();
    let app = build_router(&config, Arc::clone(&rate_limiter.limiter)).with_state(state);

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
    .with_graceful_shutdown(async move {
        shutdown::signal().await;
        // Event streams never finish on their own; ending them here is what lets the
        // drain below complete instead of waiting for Docker's kill timeout.
        shutdown.cancel();
    })
    .await
    .context("http server failed")?;

    fleet.shutdown().await;
    event!(name: "fleet.shutdown.complete", Level::INFO, "fleet watchers stopped");

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

/// The mail services. The broker and Stalwart share one HTTP client.
fn build_mail(
    config: &Config,
    database: Pool,
    cipher: Arc<Cipher>,
    events: EventBus,
) -> Result<Mail> {
    let http = reqwest::Client::builder()
        .user_agent(concat!("elysium-api/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("cannot build the mail HTTP client")?;
    let mail_config = &config.mail;

    let broker = mail_config.oauth_broker.clone().map(|public_url| {
        let internal_url = mail_config
            .oauth_broker_internal
            .clone()
            .unwrap_or_else(|| public_url.clone());
        Broker::new(http.clone(), public_url, internal_url)
    });
    let stalwart = Stalwart::new(http);

    // Connecting is lazy: nothing reaches Docker until the mail server is needed.
    let docker = Docker::connect_with_http(
        mail_config.docker.as_str().trim_end_matches('/'),
        DOCKER_TIMEOUT_SECONDS,
        API_DEFAULT_VERSION,
    )
    .context("DOCKER_URL is invalid")?;
    let hosting = Hosting::new(
        database,
        cipher,
        events,
        stalwart.clone(),
        StalwartContainer::new(docker),
        mail_config.ingress_address,
    );
    let dns = DnsChecker::from_system().context("cannot read the resolver configuration")?;

    event!(
        name: "mail.services.configured",
        Level::INFO,
        mail.broker = broker.is_some(),
        mail.docker.url = %mail_config.docker,
        mail.ingress.address = ?mail_config.ingress_address,
        "mail services configured",
    );
    Ok(Mail {
        broker,
        stalwart,
        hosting,
        dns,
    })
}

/// Assembles middleware around the routes. Outermost layers run first on the way
/// in and last on the way out, so request ids exist before anything logs, and the
/// security headers cover every response including rate limit rejections.
fn build_router(
    config: &Config,
    rate_limit: Arc<middleware::rate_limit::Limiter>,
) -> Router<AppState> {
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
        .layer(axum::middleware::from_fn_with_state(
            rate_limit,
            middleware::rate_limit::enforce,
        ));

    routes::router().layer(layers)
}
