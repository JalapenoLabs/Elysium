// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/studio-items/{id}/assets/{assetId}/content`: one of an item's files.
//! `?download=true` has the browser save it under the name the agent gave it.

use anyhow::Context;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use serde::Deserialize;
use uuid::Uuid;

use super::{StoredFile, stream_stored_file};
use crate::errors::ApiError;
use crate::models::{studio_asset, studio_item};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadQuery {
    #[serde(default)]
    download: bool,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
    query: Result<Query<ReadQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Path((id, asset_id)) = path?;
    let Query(query) = query?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let item = studio_item::find(&mut connection, id).await?;
    let asset = studio_asset::find_for_item(&mut connection, id, asset_id).await?;
    drop(connection);

    let name = asset
        .artifact_path
        .rsplit('/')
        .next()
        .unwrap_or(&asset.artifact_path);
    stream_stored_file(
        &state,
        &item,
        StoredFile {
            storage_path: &asset.storage_path,
            content_type: asset.content_type.as_deref(),
            download_as: query.download.then_some(name),
        },
    )
    .await
}
