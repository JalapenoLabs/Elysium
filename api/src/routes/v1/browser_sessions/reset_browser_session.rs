// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/browser-sessions/{id}/reset`: empty a browser's profile, signing it out of
//! everything it was signed in to on this host, and start it again at its start page.
//!
//! The browser keeps its id, port, and the connector account reading through it; only its
//! cookies, storage, and every other trace of its sign-ins go. It answers `starting`, and
//! `browserSession.upserted` reports when it runs again.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

use super::BrowserSessionResponse;
use crate::auth::CurrentUser;
use crate::browser::ResetError;
use crate::errors::ApiError;
use crate::models::browser_session;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let session = browser_session::find_owned(&mut connection, id, current.id()).await?;
    drop(connection);

    state
        .browsers
        .reset(&session)
        .await
        .map_err(|error| match error {
            ResetError::Docker(error) => ApiError::BadGateway(error.to_string()),
            ResetError::Internal(error) => ApiError::Internal(error),
        })?;

    let status = state.browsers.status(session.id);
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "browserSession": BrowserSessionResponse::new(session, status) })),
    ))
}
