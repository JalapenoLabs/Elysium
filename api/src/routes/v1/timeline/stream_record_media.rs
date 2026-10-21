// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/timeline/{id}/media`: the clip a record carries, such as a meeting segment's
//! trimmed recording, streamed from its storage location.
//!
//! Only the record's owner can play it, and no provider URL or credential reaches the
//! browser. A clip is written once under a path that names its meeting and segment, so the
//! response is cached for good, privately.

use anyhow::Context;
use axum::body::Body;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::{storage_location, timeline_record};
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
    let access_key = location
        .access_key(&state.cipher)
        .context("the storage location's access key cannot be decrypted")?;
    let download = state
        .storage
        .download(&location, &access_key, &clip.path)
        .await?;

    let mut response = Body::from_stream(download.body).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&clip.content_type)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    if let Some(length) = download.content_length {
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    }
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=31536000, immutable"),
    );
    Ok(response)
}
