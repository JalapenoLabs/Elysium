// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/studio-items/{id}`: rename an item, or pin or unpin its thumbnail.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::StudioItemResponse;
use crate::errors::ApiError;
use crate::models::studio_asset::{self, StudioAssetKind};
use crate::models::studio_item;
use crate::realtime::ServerEvent;
use crate::routes::v1::coding_sessions::validate_not_blank;
use crate::state::AppState;

/// Absent fields stay as they are. `thumbnailAssetId: null` unpins, which is why that field
/// distinguishes absent from null.
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 200), custom(function = "validate_not_blank"))]
    title: Option<String>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[expect(
        clippy::option_option,
        reason = "absent, null, and an id are three distinct requests"
    )]
    thumbnail_asset_id: Option<Option<Uuid>>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    body.validate()?;
    if body.title.is_none() && body.thumbnail_asset_id.is_none() {
        return Err(ApiError::BadRequest(
            "request body contains no fields to update".to_owned(),
        ));
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let mut item = studio_item::find(&mut connection, id).await?;
    if let Some(title) = &body.title {
        item = studio_item::rename(&mut connection, id, title.trim()).await?;
    }
    if let Some(pinned) = body.thumbnail_asset_id {
        if let Some(asset_id) = pinned {
            let asset = studio_asset::find_for_item(&mut connection, id, asset_id)
                .await
                .map_err(|error| match error {
                    diesel::result::Error::NotFound => ApiError::BadRequest(
                        "the thumbnail must be one of the item's files".to_owned(),
                    ),
                    other => other.into(),
                })?;
            if asset.kind != StudioAssetKind::Image {
                return Err(ApiError::BadRequest(
                    "the thumbnail must be an image".to_owned(),
                ));
            }
        }
        item = studio_item::pin_thumbnail(&mut connection, id, pinned).await?;
    }
    let assets = studio_asset::list_for_items(&mut connection, &[id]).await?;
    drop(connection);

    let asset_references: Vec<_> = assets.iter().collect();
    let response = StudioItemResponse::new(item, &asset_references);
    state
        .events
        .publish(&ServerEvent::StudioItemUpserted(response.clone()));
    Ok(Json(json!({ "item": response })))
}
