// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/storage-locations/{id}`: one storage location.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::StorageLocationResponse;
use crate::errors::ApiError;
use crate::models::storage_location;
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

    let location = storage_location::find(&mut connection, id).await?;

    Ok(Json(
        json!({ "location": StorageLocationResponse::new(location) }),
    ))
}
