// Copyright © 2026 Jalapeno Labs

//! `/api/v1/studio-items`: single assets made by agents, and everything they produced.
//!
//! An item runs on coding sessions (see `crate::studio`). These routes create and continue
//! them, serve the files their agents produced, and take prompts with or without a drawing.
//! See `docs/studio.md`.

mod create_studio_item;
mod delete_studio_item;
mod get_studio_item;
mod list_studio_items;
mod read_asset;
mod read_feedback_image;
mod restore_studio_item;
mod send_turn;
pub mod thread;
mod update_studio_item;

use std::collections::{HashMap, HashSet};

use anyhow::Context;
use axum::Router;
use axum::body::Body;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::coding_session::CodingSession;
use crate::models::storage_location;
use crate::models::studio_asset::{StudioAsset, StudioAssetKind};
use crate::models::studio_feedback::StudioFeedback;
use crate::models::studio_item::StudioItem;
use crate::routes::v1::coding_sessions::CodingSessionResponse;
use crate::state::AppState;
use crate::studio::thumbnail::thumbnail_of;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(list_studio_items::handle).post(create_studio_item::handle),
        )
        .route(
            "/{id}",
            get(get_studio_item::handle)
                .patch(update_studio_item::handle)
                .delete(delete_studio_item::handle),
        )
        .route("/{id}/restore", post(restore_studio_item::handle))
        .route(
            "/{id}/turns",
            // A drawn prompt carries two images, which outgrow the API-wide body limit; the
            // route checks each part against its own limit as it reads it.
            post(send_turn::handle).layer(DefaultBodyLimit::max(send_turn::MAX_BODY_BYTES)),
        )
        .route("/{id}/assets/{asset_id}/content", get(read_asset::handle))
        .route(
            "/{id}/feedback/{feedback_id}/{image}",
            get(read_feedback_image::handle),
        )
}

/// An item as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioItemResponse {
    id: Uuid,
    title: String,
    prompt: String,
    project_id: Option<Uuid>,
    storage_location_id: Uuid,
    /// The image the tile shows: the pinned one, or the default (`crate::studio::thumbnail`).
    thumbnail_asset_id: Option<Uuid>,
    /// The pinned image, or `null` when the default is shown.
    pinned_asset_id: Option<Uuid>,
    /// Distinct image paths the item holds, counting each path once however many versions.
    image_count: usize,
    /// Distinct models: files of one model share a path without its extension.
    model_count: usize,
    /// Why the latest file an agent delivered could not be kept, such as the storage limit.
    pull_error: Option<String>,
    deleted_at: Option<DateTime<Utc>>,
    /// Who created it.
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl StudioItemResponse {
    /// `assets` are the item's files, newest first.
    pub fn new(item: StudioItem, assets: &[&StudioAsset]) -> Self {
        let mut image_paths = HashSet::new();
        let mut model_stems = HashSet::new();
        for asset in assets {
            match asset.kind {
                StudioAssetKind::Image => {
                    image_paths.insert(asset.artifact_path.as_str());
                }
                StudioAssetKind::Model => {
                    let path = asset.artifact_path.as_str();
                    let stem = path
                        .rsplit_once('.')
                        .map_or(path, |(stem, _extension)| stem);
                    model_stems.insert(stem);
                }
                StudioAssetKind::File => {}
            }
        }

        Self {
            thumbnail_asset_id: thumbnail_of(item.thumbnail_asset_id, assets),
            pinned_asset_id: item.thumbnail_asset_id,
            image_count: image_paths.len(),
            model_count: model_stems.len(),
            pull_error: item.pull_error,
            id: item.id,
            title: item.title,
            prompt: item.prompt,
            project_id: item.project_id,
            storage_location_id: item.storage_location_id,
            deleted_at: item.deleted_at,
            created_by: item.created_by,
            created_at: item.created_at,
            updated_at: item.updated_at,
        }
    }

    /// Responses for `items`, each with its own files picked out of `assets` (newest first)
    /// in one pass.
    pub fn many(items: Vec<StudioItem>, assets: &[StudioAsset]) -> Vec<Self> {
        let mut assets_by_item: HashMap<Uuid, Vec<&StudioAsset>> = HashMap::new();
        for asset in assets {
            assets_by_item
                .entry(asset.studio_item_id)
                .or_default()
                .push(asset);
        }
        items
            .into_iter()
            .map(|item| {
                let item_assets = assets_by_item.remove(&item.id).unwrap_or_default();
                Self::new(item, &item_assets)
            })
            .collect()
    }
}

/// A file as clients see it. Its bytes are at
/// `/api/v1/studio-items/{studioItemId}/assets/{id}/content`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioAssetResponse {
    id: Uuid,
    studio_item_id: Uuid,
    session_id: Option<i64>,
    kind: StudioAssetKind,
    /// Where the agent wrote it, relative to `artifacts/`.
    artifact_path: String,
    /// The file's name, for saving it.
    name: String,
    content_type: Option<String>,
    size_bytes: i64,
    sha256: String,
    created_at: DateTime<Utc>,
}

impl From<StudioAsset> for StudioAssetResponse {
    fn from(asset: StudioAsset) -> Self {
        let name = asset
            .artifact_path
            .rsplit('/')
            .next()
            .unwrap_or(&asset.artifact_path)
            .to_owned();
        Self {
            id: asset.id,
            studio_item_id: asset.studio_item_id,
            session_id: asset.session_id,
            kind: asset.kind,
            artifact_path: asset.artifact_path,
            name,
            content_type: asset.content_type,
            size_bytes: asset.size_bytes,
            sha256: asset.sha256,
            created_at: asset.created_at,
        }
    }
}

/// A drawn prompt as clients see it. Its images are at
/// `/api/v1/studio-items/{studioItemId}/feedback/{id}/annotated` and `.../capture`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioFeedbackResponse {
    id: Uuid,
    studio_item_id: Uuid,
    session_id: Option<i64>,
    turn_id: Option<String>,
    prompt: String,
    source_asset_id: Option<Uuid>,
    camera_orbit: Option<String>,
    /// Whether the clean view was kept beside the drawing.
    has_capture: bool,
    /// Who sent it.
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl From<StudioFeedback> for StudioFeedbackResponse {
    fn from(feedback: StudioFeedback) -> Self {
        Self {
            id: feedback.id,
            studio_item_id: feedback.studio_item_id,
            session_id: feedback.session_id,
            turn_id: feedback.turn_id,
            prompt: feedback.prompt,
            source_asset_id: feedback.source_asset_id,
            camera_orbit: feedback.camera_orbit,
            has_capture: feedback.capture_storage_path.is_some(),
            created_by: feedback.created_by,
            created_at: feedback.created_at,
        }
    }
}

/// An item's sessions as clients see them, with their threads' last known state.
pub fn session_responses(
    state: &AppState,
    sessions: Vec<CodingSession>,
) -> Vec<CodingSessionResponse> {
    sessions
        .into_iter()
        .map(|session| {
            let thread = state.fleet.thread_status(session.id);
            CodingSessionResponse::new(session, thread)
        })
        .collect()
}

/// How a stored file is handed to the browser.
#[derive(Debug)]
pub struct StoredFile<'file> {
    /// Where it is kept, relative to the item's storage location.
    pub storage_path: &'file str,
    /// The media type Elysium recorded, if any; otherwise the provider's.
    pub content_type: Option<&'file str>,
    /// `Some(name)` to have the browser save it under that name rather than show it.
    pub download_as: Option<&'file str>,
}

/// Streams one of an item's files from its storage location. No provider URL or credential
/// reaches the browser.
///
/// Files are addressed by content, so a response is cached for good. An agent wrote the bytes,
/// so the response is sandboxed and never sniffed: an SVG opened directly runs no script.
///
/// # Errors
/// Answers `404` when the provider holds no such file, and `502` when it refuses.
pub async fn stream_stored_file(
    state: &AppState,
    item: &StudioItem,
    file: StoredFile<'_>,
) -> Result<Response, ApiError> {
    let location = {
        let mut connection = state
            .database
            .get()
            .await
            .context("no database connection available")?;
        storage_location::find(&mut connection, item.storage_location_id).await?
    };
    let access_key = location
        .access_key(&state.cipher)
        .context("the storage location's access key cannot be decrypted")?;
    let download = state
        .storage
        .download(&location, &access_key, file.storage_path)
        .await?;

    let content_type = file
        .content_type
        .map(ToOwned::to_owned)
        .or(download.content_type)
        .unwrap_or_else(|| "application/octet-stream".to_owned());
    let mut response = Body::from_stream(download.body).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&content_type)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    if let Some(length) = download.content_length {
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    }
    // Private: an item's files are workspace data, not for shared caches.
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=31536000, immutable"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'; style-src 'unsafe-inline'"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if let Some(name) = file.download_as {
        // An agent chose the name, so only plain characters reach the header.
        let safe_name: String = name
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                    character
                } else {
                    '_'
                }
            })
            .collect();
        let disposition = format!("attachment; filename=\"{safe_name}\"");
        if let Ok(value) = HeaderValue::from_str(&disposition) {
            headers.insert(header::CONTENT_DISPOSITION, value);
        }
    }
    Ok(response)
}
