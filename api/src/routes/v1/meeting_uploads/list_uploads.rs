// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/meeting-uploads`: the signed-in person's unfinished uploads, the most recently
//! moved first, so the upload page resumes one when the same files are chosen again.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::UploadResponse;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::meeting_upload;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let uploads: Vec<UploadResponse> =
        meeting_upload::list_unfinished(&mut connection, current.id())
            .await?
            .into_iter()
            .map(UploadResponse::from)
            .collect();
    Ok(Json(json!({ "uploads": uploads })))
}
