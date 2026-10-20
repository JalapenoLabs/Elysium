// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/studio-items/{id}/feedback/{feedbackId}/{image}`: a drawn prompt's images,
//! `annotated` (the drawing as sent) or `capture` (the view without it).

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::response::Response;
use serde::Deserialize;
use uuid::Uuid;

use super::{StoredFile, stream_stored_file};
use crate::errors::ApiError;
use crate::models::{studio_feedback, studio_item};
use crate::state::AppState;

/// Which of a drawn prompt's two images.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackImage {
    Annotated,
    Capture,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, Uuid, FeedbackImage)>, PathRejection>,
) -> Result<Response, ApiError> {
    let Path((id, feedback_id, image)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let item = studio_item::find(&mut connection, id).await?;
    let feedback = studio_feedback::find_for_item(&mut connection, id, feedback_id).await?;
    drop(connection);

    let storage_path = match image {
        FeedbackImage::Annotated => Some(feedback.annotated_storage_path.as_str()),
        FeedbackImage::Capture => feedback.capture_storage_path.as_deref(),
    };
    let Some(storage_path) = storage_path else {
        return Err(ApiError::NotFound);
    };
    stream_stored_file(
        &state,
        &item,
        StoredFile {
            storage_path,
            content_type: Some("image/png"),
            download_as: None,
        },
    )
    .await
}
