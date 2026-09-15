// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/projects/{id}/cover`: replace a project's cover image.
//!
//! See [`crate::images::MAX_UPLOAD_BYTES`].
//!
//! The body is the image file itself, up to [`MAX_UPLOAD_BYTES`] (the API's 1 MiB body limit
//! sits just above it); its content type is ignored, since the format is read from the bytes. The image is compressed before it is
//! stored, see [`crate::images`].

use anyhow::Context;
use axum::Json;
use axum::body::Bytes;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::ProjectResponse;
use crate::errors::ApiError;
use crate::images::compress_cover;
use crate::models::project;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    if body.is_empty() {
        return Err(ApiError::BadRequest(
            "the request body must be the image file".to_owned(),
        ));
    }

    // Decoding and encoding take tens of milliseconds of CPU; keep them off the runtime.
    let compressed = tokio::task::spawn_blocking(move || compress_cover(&body))
        .await
        .context("the cover compression task panicked")??;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let updated = project::set_cover(&mut connection, id, Some(compressed)).await?;

    let response = ProjectResponse::from(updated);
    let body = json!({ "project": &response });
    state
        .events
        .publish(&ServerEvent::ProjectUpserted(response));
    Ok(Json(body))
}
