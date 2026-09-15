// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/storage-locations/{id}/test`: reach the location with its saved
//! settings right now.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

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
    drop(connection);

    let access_key = location
        .access_key(&state.cipher)
        .context("the stored access key cannot be decrypted")?;
    let entries = state.storage.check(&location, &access_key).await?;

    Ok(Json(json!({ "result": { "entries": entries } })))
}
