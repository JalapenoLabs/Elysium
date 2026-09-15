// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/storage-locations`: every storage location.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::StorageLocationResponse;
use crate::errors::ApiError;
use crate::models::storage_location;
use crate::state::AppState;

pub async fn handle(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let locations: Vec<StorageLocationResponse> = storage_location::list(&mut connection)
        .await?
        .into_iter()
        .map(StorageLocationResponse::new)
        .collect();

    Ok(Json(json!({ "locations": locations })))
}
