// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/meeting-uploads/{id}`: stops one of the signed-in person's unfinished uploads
//! and removes what of it arrived. A finished upload is its meeting's now, and answers `404`.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use tracing::{Level, event};
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::meeting_upload;
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
    if !meeting_upload::delete_unfinished(&mut connection, id, current.id()).await? {
        return Err(ApiError::NotFound);
    }
    drop(connection);
    // The row is gone, so nothing reads its parts again; parts left behind cost the workspace
    // space and nothing else, and are logged rather than refusing what was asked.
    if let Err(error) = state.upload_staging.remove(id).await {
        event!(
            name: "meeting_uploads.staging.unremoved",
            Level::WARN,
            meeting_upload.id = %id,
            error.message = %error,
            "a cancelled upload's parts could not be removed",
        );
    }
    Ok(StatusCode::NO_CONTENT)
}
