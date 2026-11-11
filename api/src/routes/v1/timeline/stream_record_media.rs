// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/timeline/{id}/media`: the clip a record carries, such as a meeting segment's
//! trimmed recording, streamed from its storage location a range at a time
//! (`routes::v1::media`).
//!
//! Only the record's owner can play it, and no provider URL or credential reaches the
//! browser.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::{storage_location, timeline_record};
use crate::routes::v1::media;
use crate::state::AppState;

/// A record's clip, as `media.clip` stores it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredClip {
    storage_location_id: Uuid,
    path: String,
    content_type: String,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let Path(id) = path?;
    let (clip, location) = {
        let mut connection = state
            .database
            .get()
            .await
            .context("no database connection available")?;
        let (record, _attributions) =
            timeline_record::find_owned(&mut connection, id, current.id()).await?;
        let Some(clip) = record
            .media
            .as_ref()
            .and_then(|media| media.get("clip"))
            .filter(|clip| !clip.is_null())
        else {
            return Err(ApiError::NotFound);
        };
        let clip: StoredClip = serde_json::from_value(clip.clone())
            .context("a record's clip is not in the stored shape")?;
        let location = storage_location::find(&mut connection, clip.storage_location_id).await?;
        (clip, location)
    };
    media::stream(&state, &location, &clip.path, &clip.content_type, &headers).await
}
