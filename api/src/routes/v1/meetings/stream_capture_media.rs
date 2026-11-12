// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/meetings/{id}/captures/{captureId}/media`: one capture's recording, for the
//! player on its meeting's page, streamed from its storage location a range at a time
//! (`routes::v1::media`), so a two hour recording seeks without being downloaded whole.
//!
//! It answers anyone who may read the meeting (`docs/meeting-review.md`, Who sees a meeting).
//! `404` for a meeting they may not read, a capture that is not the meeting's, or one with no
//! recording kept.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::access::Need;
use crate::meetings::media::media_type_of;
use crate::models::{meeting_capture, storage_location};
use crate::routes::v1::media;
use crate::routes::v1::meetings::open;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let Path((id, capture_id)) = path?;
    let (recording, location) = {
        let mut connection = state
            .database
            .get()
            .await
            .context("no database connection available")?;
        let (found, _access) = open(&mut connection, id, &current, Need::Read).await?;
        let capture = meeting_capture::find(&mut connection, capture_id).await?;
        if capture.meeting_id != found.id {
            return Err(ApiError::NotFound);
        }
        let Some(recording) = capture.stored_recording() else {
            return Err(ApiError::NotFound);
        };
        let location =
            storage_location::find(&mut connection, recording.storage_location_id).await?;
        (recording, location)
    };
    // A recording kept before its type was recorded is named by its extension.
    let content_type = recording
        .content_type
        .as_deref()
        .or_else(|| media_type_of(&recording.path))
        .unwrap_or("application/octet-stream");
    media::stream(&state, &location, &recording.path, content_type, &headers).await
}
