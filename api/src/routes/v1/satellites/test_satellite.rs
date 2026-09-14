// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/satellites/{id}/test`: connect with the stored settings right now.
//!
//! Uses a fresh connection rather than the fleet's cached client, so the answer
//! reflects the URL and secret as saved, including for an inactive satellite.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::errors::ApiError;
use crate::fleet;
use crate::models::satellite;
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
    let satellite = satellite::find(&mut connection, id).await?;
    drop(connection);

    let client = fleet::connect(&satellite, &state.cipher).await?;
    let version = client.version().await?;
    let status = client.status().await?;

    Ok(Json(json!({
        "result": {
            "version": version.satellite_version,
            "protoMajor": version.proto_major,
            "protoMinor": version.proto_minor,
            "runningThreads": status.running_threads,
            "maxConcurrentThreads": status.max_concurrent_threads,
        }
    })))
}
