// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/satellites/{id}`: one satellite with its latest status.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::SatelliteResponse;
use crate::errors::ApiError;
use crate::models::satellite;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let satellite = find(&state, id).await?;
    Ok(Json(json!({ "satellite": satellite })))
}

/// The satellite `id`, with the fleet's latest status.
///
/// # Errors
/// `404` for an unknown satellite.
pub async fn find(state: &AppState, id: Uuid) -> Result<SatelliteResponse, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let satellite = satellite::find(&mut connection, id).await?;
    let status = state.fleet.satellite_status(id);
    Ok(SatelliteResponse::new(satellite, status))
}
