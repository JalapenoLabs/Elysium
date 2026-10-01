// Copyright © 2026 Jalapeno Labs

//! The HTTP server: startup, middleware stack, and graceful shutdown.
//!
//! Startup order: configuration, encryption key, schema check, store connections
//! (with retry), fleet watchers, the link watcher, router, listener. Shutdown order is the
//! reverse: a signal cancels the shutdown token, which ends every event stream, fleet
//! watcher, and the link watcher; the server stops accepting and drains in-flight
//! requests; the watchers are awaited; then the Postgres pool closes and the Redis
//! connection drops.

use std::net::SocketAddr;
use std::path::Path;
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

use crate::action_items::links::Links;
use crate::action_items::watcher::{WatchContext, Watcher};
use crate::auth::kratos::Kratos;
use crate::auth::{Auth, hook_key};
use crate::config::Config;
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::database::migrations;
use crate::fleet::Fleet;
use crate::github::Github;
use crate::jira::Jira;
use crate::mail::Mail;
use crate::mail::broker::Broker;
use crate::mail::dns::DnsChecker;
use crate::mail::hosting::Hosting;
use crate::mail::stalwart::Stalwart;
use crate::mail::stalwart::container::StalwartContainer;
use crate::realtime::EventBus;
use crate::state::AppState;
use crate::storage::Storage;
use crate::version::VersionInfo;
use crate::{connections, middleware, routes, shutdown, web_app};

/// How long one Docker call may take. Pulling the mail server's image is one call, and
/// can take minutes on a slow connection.
const DOCKER_TIMEOUT_SECONDS: u64 = 600;

/// Serves the API until a shutdown signal arrives.
///
/// # Errors
/// Fails on invalid configuration, an invalid encryption key, pending migrations,
/// unreachable stores, or a listener that cannot bind.
#[expect(
    clippy::too_many_lines,
    reason = "startup is one ordered sequence, and reads best top to bottom in one place"
)]
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

    // One HTTP client for every outbound call: the OAuth broker, Stalwart, GitHub, and
    // storage providers share its connection pool.
    let http = reqwest::Client::builder()
        .user_agent(concat!("elysium-api/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("cannot build the HTTP client")?;
    // Built before the fleet starts, because the fleet's relays answer agents' storage
    // tool calls with it.
    let storage = Storage::new(http.clone());

    let github = Github::new(http.clone());
    let jira = Jira::new(http.clone());
    // Built before the fleet too: an agent links its pull request through it.
    let links = Links::new(database.clone(), Arc::clone(&cipher), &jira, &github);

    let shutdown = CancellationToken::new();
    let events = EventBus::new();
    let fleet = Fleet::new(
        database.clone(),
        Arc::clone(&cipher),
        events.clone(),
        storage.clone(),
        links.clone(),
        shutdown.clone(),
    );
    fleet.start().await?;
    let watch = WatchContext {
        database: database.clone(),
        events: events.clone(),
        links: links.clone(),
    };
    let watcher = Watcher::start(watch, shutdown.clone());
    let mail = build_mail(
        &config,
        http.clone(),
        database.clone(),
        Arc::clone(&cipher),
        events.clone(),
    )?;
    // Brings an existing mail server back up; pulling its image may take a while, so
    // the API does not wait for it.
    let hosting = mail.hosting.clone();
    tokio::spawn(async move { hosting.reconcile().await });

    let state = AppState {
        auth: build_auth(&config, http.clone()),
        database: database.clone(),
        redis: redis.clone(),
        cipher,
        version,
        events,
        fleet: fleet.clone(),
        github,
        jira,
        links,
        mail,
        storage,
        shutdown: shutdown.clone(),
    };
    let rate_limiter = middleware::rate_limit::build();
    let app = build_router(&config, Arc::clone(&rate_limiter.limiter), &state)?.with_state(state);

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
    watcher.stopped().await;

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

/// Kratos, and what requests are checked against. See `crate::auth`.
pub(crate) fn build_auth(config: &Config, http: reqwest::Client) -> Auth {
    Auth {
        kratos: Kratos::new(
            http,
            config.kratos_public_url.clone(),
            config.kratos_admin_url.clone(),
        ),
        hook_key: hook_key::derive(&config.encryption_key),
        public_url: config.public_url.clone(),
    }
}

/// The mail services. The broker and Stalwart share `http`.
pub(crate) fn build_mail(
    config: &Config,
    http: reqwest::Client,
    database: Pool,
    cipher: Arc<Cipher>,
    events: EventBus,
) -> Result<Mail> {
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

/// Assembles middleware around the routes and, when the image carries one, the web
/// app. Outermost layers run first on the way in and last on the way out, so request
/// ids exist before anything logs, and the security headers cover every response
/// including rate limit rejections and the web app's files.
///
/// Only `/api` is rate limited. A cold page load fetches several of the web app's files
/// at once, and serving one costs a file read, so limiting them would spend a visitor's
/// API budget on its first paint without protecting anything.
///
/// # Errors
/// Fails when `FRONTEND_DIR` names a directory without a build in it.
fn build_router(
    config: &Config,
    rate_limit: Arc<middleware::rate_limit::Limiter>,
    state: &AppState,
) -> Result<Router<AppState>> {
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
        .layer(middleware::cors::layer(config.cors_allowed_origins.clone()));

    let routes = assemble_routes(
        routes::router(state),
        rate_limit,
        config.frontend_dir.as_deref(),
    )?;
    Ok(routes.layer(layers))
}

/// Rate limits `api`, then merges the web app beside it when `frontend_dir` names one.
///
/// The order is load-bearing. The limiter layer wraps `api`'s routes and its default
/// fallback; merging replaces that fallback with the web app's, which therefore never
/// passes through the limiter.
///
/// # Errors
/// Fails when `frontend_dir` names a directory without a build in it.
fn assemble_routes<State>(
    api: Router<State>,
    rate_limit: Arc<middleware::rate_limit::Limiter>,
    frontend_dir: Option<&Path>,
) -> Result<Router<State>>
where
    State: Clone + Send + Sync + 'static,
{
    let api = api.layer(axum::middleware::from_fn_with_state(
        rate_limit,
        middleware::rate_limit::enforce,
    ));

    let Some(directory) = frontend_dir else {
        event!(
            name: "api.web_app.disabled",
            Level::INFO,
            "FRONTEND_DIR is unset, so only /api is served",
        );
        return Ok(api);
    };

    let app = api.merge(web_app::router(directory)?);
    event!(
        name: "api.web_app.configured",
        Level::INFO,
        file.directory = %directory.display(),
        "serving the web app from {{file.directory}}",
    );
    Ok(app)
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::get;
    use tower::ServiceExt;

    use super::*;
    use crate::middleware::rate_limit::BURST_SIZE;

    async fn status_of(app: &Router, path: &str) -> StatusCode {
        let request = Request::get(path)
            .header("x-real-ip", "203.0.113.7")
            .body(Body::empty())
            .expect("request");
        app.clone()
            .oneshot(request)
            .await
            .expect("infallible")
            .status()
    }

    // The web app's files skip the limiter, which is what keeps a cold page load from
    // spending a visitor's API budget, while /api stays limited. Both follow from the
    // layer-then-merge order in `assemble_routes`, which nothing else would catch.
    #[tokio::test]
    async fn only_api_is_rate_limited_beside_the_web_app() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(directory.path().join("index.html"), "<!doctype html>").expect("index.html");
        let api = Router::new().nest("/api", Router::new().route("/ok", get(|| async { "ok" })));
        let rate_limiter = middleware::rate_limit::build();
        let app = assemble_routes(
            api,
            Arc::clone(&rate_limiter.limiter),
            Some(directory.path()),
        )
        .expect("a valid build");

        for _ in 0..=BURST_SIZE {
            assert_eq!(status_of(&app, "/projects").await, StatusCode::OK);
        }

        for _ in 0..BURST_SIZE {
            assert_eq!(status_of(&app, "/api/ok").await, StatusCode::OK);
        }
        assert_eq!(
            status_of(&app, "/api/ok").await,
            StatusCode::TOO_MANY_REQUESTS
        );
        rate_limiter.sweeper.abort();
    }
}
