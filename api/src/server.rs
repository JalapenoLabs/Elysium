// Copyright © 2026 Jalapeno Labs

//! The HTTP server: startup, middleware stack, and graceful shutdown.
//!
//! The API serves clients alone, the web app, its `/api`, and MCP clients, and runs no
//! background work: that is the engine's (`crate::engine`). Whatever a request starts that
//! outlives it, it records in Postgres for the engine to take, and signals the engine on the bus.
//!
//! Startup order: the shared foundation (`crate::services::Foundation`: configuration,
//! encryption key, schema check, store connections, and the event bus's subscriptions), the
//! services requests reach, router, listener. Shutdown order is the reverse: a signal cancels
//! the shutdown token, which ends every event stream; the server stops accepting and drains
//! in-flight requests; then the event bus flushes what they published last, the Postgres pool
//! closes, and the Redis connection drops.

use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tower::ServiceBuilder;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::timeout::TimeoutLayer;
use tracing::{Level, event};

use crate::backups::{self, Backups, BackupsDeps};
use crate::browser::devtools::Devtools;
use crate::claude_accounts::oauth::Anthropic;
use crate::claude_accounts::{ClaudeAccounts, ClaudeAccountsDeps, TurnSignal};
use crate::config::Config;
use crate::confluence::Confluence;
use crate::confluence::reader::{Reader as ConfluenceReader, ReaderDeps as ConfluenceReaderDeps};
use crate::connectors::Connectors;
use crate::connectors::activity::Activity;
use crate::connectors::checks::CheckRuns;
use crate::connectors::ingest::Ingest;
use crate::issue_reports::IssueReports;
use crate::knowledge::search::KnowledgeSearch;
use crate::meetings::sources::Sources;
use crate::passwords::remote::RemoteVault;
use crate::services::{self, Foundation};
use crate::sessions::{Sessions, SessionsDeps};
use crate::state::AppState;
use crate::tools::grants::Grants;
use crate::updates::{Updates, UpdatesDeps};
use crate::youtrack::Youtrack;
use crate::youtrack::reader::{ReaderDeps, YoutrackReader};
use crate::{middleware, routes, search, web_app};

/// Serves the API until a shutdown signal arrives.
///
/// # Errors
/// Fails on invalid configuration, an invalid encryption key, pending migrations,
/// unreachable stores, or a listener that cannot bind.
pub async fn serve() -> Result<()> {
    let foundation = Foundation::connect("elysium-api").await?;
    let config = Arc::clone(&foundation.config);
    let (state, vault_replies) = build_state(&foundation)?;

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

    let signalled = foundation.clone();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    // Event streams never finish on their own; cancelling the shutdown token ends them, which
    // is what lets the drain complete instead of waiting for Docker's kill timeout.
    .with_graceful_shutdown(async move { signalled.shutdown_signal().await })
    .await
    .context("http server failed")?;

    // serve() returning means every in-flight request has finished and the router, with its
    // copies of the handles, is gone.
    rate_limiter.sweeper.abort();
    if let Err(error) = vault_replies.await {
        event!(
            name: "passwords.remote.join_failed",
            Level::ERROR,
            error.message = %error,
            "the password vault's answers ended abnormally",
        );
    }
    foundation.close().await;
    event!(name: "api.shutdown.complete", Level::INFO, "shutdown complete");
    Ok(())
}

/// Everything a request reaches, on `foundation`, and the task that hands the password vault's
/// answers to the requests waiting for them, which ends with the shutdown token.
///
/// # Errors
/// Fails when `DOCKER_URL` is not a URL, or a client cannot be built.
#[expect(
    clippy::too_many_lines,
    reason = "one field per service a request reaches, read best as the one struct it builds"
)]
pub(crate) fn build_state(foundation: &Foundation) -> Result<(AppState, JoinHandle<()>)> {
    let config = &foundation.config;
    let database = &foundation.database;
    let cipher = &foundation.cipher;
    let events = &foundation.events;
    let http = &foundation.http;

    let workspace = services::build_workspace(config)?;
    let connector_activity = Activity::new(database.clone(), events.clone());
    let (passwords, vault_replies) = RemoteVault::start(
        events.clone(),
        Arc::clone(cipher),
        foundation.shutdown.clone(),
    );
    let state = AppState {
        auth: services::build_auth(config, http.clone()),
        backups: Backups::new(BackupsDeps {
            database: database.clone(),
            cipher: Arc::clone(cipher),
            storage: foundation.storage.clone(),
            events: events.clone(),
            // The API holds no work of its own; a backup it asks for runs in the engine.
            activity: crate::activity::Activities::new(),
            dumper: services::build_dumper(config)?,
            staging_directory: backups::STAGING_DIRECTORY.into(),
            shutdown: foundation.shutdown.clone(),
        }),
        browsers: services::build_browsers(
            config,
            http.clone(),
            database.clone(),
            Arc::clone(cipher),
            events.clone(),
        )?,
        claude_accounts: ClaudeAccounts::new(ClaudeAccountsDeps {
            database: database.clone(),
            cipher: Arc::clone(cipher),
            events: events.clone(),
            anthropic: Anthropic::new(http.clone()),
            workspace: workspace.clone(),
            // Turns finish in the engine, whose keeper hears them; the API's never rings.
            turn_signal: TurnSignal::default(),
        }),
        connectors: Connectors {
            ingest: Ingest::new(
                database.clone(),
                events.clone(),
                Devtools::new(http.clone()),
                connector_activity.clone(),
            ),
            confluence: ConfluenceReader::new(ConfluenceReaderDeps {
                database: database.clone(),
                cipher: Arc::clone(cipher),
                events: events.clone(),
                activity: connector_activity.clone(),
                confluence: Confluence::new(http.clone()),
            }),
            youtrack: YoutrackReader::new(ReaderDeps {
                database: database.clone(),
                events: events.clone(),
                cipher: Arc::clone(cipher),
                youtrack: Youtrack::new(http.clone()),
                activity: connector_activity.clone(),
            }),
            activity: connector_activity,
            check_runs: CheckRuns::new(events.clone()),
        },
        database: database.clone(),
        redis: foundation.redis.clone(),
        cipher: Arc::clone(cipher),
        version: Arc::clone(&foundation.version),
        events: events.clone(),
        github: foundation.github.clone(),
        grants: Grants::new(database.clone()),
        issue_reports: IssueReports::default(),
        jira: foundation.jira.clone(),
        knowledge: KnowledgeSearch::new(foundation.embeddings.clone()),
        links: foundation.links.clone(),
        mail: services::build_mail(
            config,
            http.clone(),
            database.clone(),
            Arc::clone(cipher),
            events.clone(),
        )?,
        meeting_sources: Sources::new(http.clone()),
        passwords,
        search_limiter: Arc::new(middleware::rate_limit::Limiter::new(search::PER_PERSON)),
        sessions: Sessions::new(SessionsDeps {
            database: database.clone(),
            cipher: Arc::clone(cipher),
            events: events.clone(),
            workspace,
        }),
        storage: foundation.storage.clone(),
        updates: Updates::new(UpdatesDeps {
            database: database.clone(),
            events: events.clone(),
            http: http.clone(),
            version: Arc::clone(&foundation.version),
            encryption_key: config.encryption_key.clone(),
        }),
        shutdown: foundation.shutdown.clone(),
    };
    Ok((state, vault_replies))
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
    rate_limit: Arc<middleware::rate_limit::Limiter<IpAddr>>,
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
    rate_limit: Arc<middleware::rate_limit::Limiter<IpAddr>>,
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
