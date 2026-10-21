// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/browser-sessions/{id}/stream`: the WebSocket a viewer negotiates a
//! browser's stream over. See `crate::browser::relay`.
//!
//! Only the browser's owner may open it, and only from Elysium's own origin: an upgrade is
//! a `GET`, which the unsafe-request check in front of every route lets through, and a page
//! on another site could otherwise open a socket with the owner's cookie. The API signs in
//! to the browser before it accepts the upgrade, so a browser that is not answering is a
//! `502` the frontend can show, not a socket that closes at once.
//!
//! The socket ends when the browser, the viewer, or the process does, and when the owner's
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
use uuid::Uuid;

use crate::auth::middleware::revoked;
use crate::auth::{AccessRefusal, CurrentUser};
use crate::browser::relay;
use crate::errors::ApiError;
use crate::models::browser_session;
use crate::state::AppState;

/// How often an open stream checks that its owner may still use the workspace, as the
/// event stream does.
const ACCESS_CHECK_INTERVAL: Duration = Duration::from_secs(30);

/// The largest message a viewer may send. Signaling messages are a few kilobytes; a pasted
/// clipboard is the largest thing a viewer sends.
const MAX_VIEWER_MESSAGE_BYTES: usize = 256 * 1024;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    headers: HeaderMap,
    path: Result<Path<Uuid>, PathRejection>,
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
            url.path = "/api/v1/browser-sessions/{id}/stream",
            origin = origin.unwrap_or("<none>"),
            "refused a browser stream from another origin",
        );
        return Err(ApiError::AccessRefused(AccessRefusal::CrossOrigin));
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let session = browser_session::find_owned(&mut connection, id, current.id()).await?;
    drop(connection);

    let upstream = state
        .browsers
        .connect(&session)
        .await
        .map_err(|error| ApiError::BadGateway(format!("{error:#}")))?;

    let shutdown = Box::pin(state.shutdown.clone().cancelled_owned());
    let access = Box::pin(revoked(state, headers, ACCESS_CHECK_INTERVAL));
    let until = async move {
        future::select(shutdown, access).await;
    };
    Ok(upgrade
        .max_message_size(MAX_VIEWER_MESSAGE_BYTES)
        .on_upgrade(move |viewer| relay::run(viewer, upstream, until)))
}
