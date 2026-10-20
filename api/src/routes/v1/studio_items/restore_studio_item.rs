// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/studio-items/{id}/restore`: bring back a softly deleted item.
//!
//! Its thread was destroyed when it was deleted, so its next prompt continues it on a new
//! one (see `docs/studio.md`, Continuing).

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::StudioItemResponse;
use crate::errors::ApiError;
use crate::models::{studio_asset, studio_item};
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let item = studio_item::set_deleted_at(&mut connection, id, None).await?;
    let assets = studio_asset::list_for_items(&mut connection, &[id]).await?;
    drop(connection);

    let asset_references: Vec<_> = assets.iter().collect();
    let response = StudioItemResponse::new(item, &asset_references);
    state
        .events
        .publish(&ServerEvent::StudioItemUpserted(response.clone()));
    Ok(Json(json!({ "item": response })))
}
