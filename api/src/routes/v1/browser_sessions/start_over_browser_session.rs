// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/browser-sessions/{id}/start-over`: send a browser back to its start page,
//! closing every other page and keeping its profile.
//!
//! The way out of a sign-in that landed on a page with nowhere to go, such as an error a
//! provider shows after its MFA was reset. Everything the browser is signed in to stays.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::browser_session;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let session = browser_session::find_owned(&mut connection, id, current.id()).await?;
    drop(connection);

    // The browser is a service of its own: one that is not answering is a bad gateway, not
    // a fault of the API's.
    state
        .browsers
        .start_over(&session)
        .await
        .map_err(|error| ApiError::BadGateway(format!("{error:#}")))?;
    Ok(StatusCode::NO_CONTENT)
}
