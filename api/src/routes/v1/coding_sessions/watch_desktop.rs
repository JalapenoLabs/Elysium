// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/coding-sessions/{id}/desktop`: the WebSocket a person watches a session's
//! running turn's desktop over, as the VNC protocol. See `crate::sessions::desktop`.
//!
//! Only from Elysium's own origin: an upgrade is a `GET`, which the unsafe-request check in front
//! of every route lets through, and a page on another site could otherwise open a socket with the
//! person's cookie. The API checks that the desktop is up before it accepts the upgrade, so a
//! session with no turn running is a `404` the frontend can show, not a socket that closes at once.
//!
//! The socket ends when the turn's desktop, the viewer, or the process does, and when the person's
//! session or account stops being allowed, checked every [`ACCESS_CHECK_INTERVAL`].

use std::time::Duration;

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, header};
use axum::response::Response;
use futures_util::future;
use tracing::{Level, event};

use crate::auth::AccessRefusal;
use crate::auth::middleware::revoked;
use crate::errors::ApiError;
use crate::models::coding_session;
use crate::sessions::desktop;
use crate::state::AppState;

/// How often an open view checks that its viewer may still use the workspace, as the event
/// stream does.
const ACCESS_CHECK_INTERVAL: Duration = Duration::from_secs(30);

/// The largest message a viewer may send. A VNC viewer's messages are a few bytes: update
/// requests and pixel formats.
const MAX_VIEWER_MESSAGE_BYTES: usize = 64 * 1024;

pub async fn handle(
    State(state): State<AppState>,
    headers: HeaderMap,
    path: Result<Path<i64>, PathRejection>,
    upgrade: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let Path(id) = path?;
    let expected_origin = state.auth.public_url.origin().ascii_serialization();
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    if origin != Some(expected_origin.as_str()) {
        event!(
            name: "auth.origin.refused",
            Level::WARN,
            url.path = "/api/v1/coding-sessions/{id}/desktop",
            origin = origin.unwrap_or("<none>"),
            "refused a desktop view from another origin",
        );
        return Err(ApiError::AccessRefused(AccessRefusal::CrossOrigin));
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    coding_session::find(&mut connection, id).await?;
    drop(connection);

    let workspace = state.sessions.workspace();
    let socket = desktop::socket(id);
    let is_up = workspace
        .has_socket(&socket)
        .await
        .map_err(|error| ApiError::BadGateway(format!("{error:#}")))?;
    if !is_up {
        return Err(ApiError::NotFound);
    }
    let desktop_connection = workspace
        .connect_socket(&socket)
        .await
        .map_err(|error| ApiError::BadGateway(format!("{error:#}")))?;

    let shutdown = Box::pin(state.shutdown.clone().cancelled_owned());
    let access = Box::pin(revoked(state, headers, ACCESS_CHECK_INTERVAL));
    let until = async move {
        future::select(shutdown, access).await;
    };
    Ok(upgrade
        .max_message_size(MAX_VIEWER_MESSAGE_BYTES)
        .on_upgrade(move |viewer| desktop::relay(viewer, desktop_connection, until)))
}
