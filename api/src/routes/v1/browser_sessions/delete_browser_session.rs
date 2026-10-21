// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/browser-sessions/{id}`: remove a browser, its container, and its
//! profile, which signs it out of everything it was signed in to on this host.
//!
//! The accounts themselves are untouched: the sessions the provider issued simply stop
//! being used. A provider that lists active sessions may still show this one until it
//! expires.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::browser::DeleteError;
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

    state
        .browsers
        .delete(&session)
        .await
        .map_err(|error| match error {
            DeleteError::Docker(error) => ApiError::BadGateway(error.to_string()),
            DeleteError::Internal(error) => ApiError::Internal(error),
        })?;
    Ok(StatusCode::NO_CONTENT)
}
