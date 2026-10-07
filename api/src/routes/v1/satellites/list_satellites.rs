// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/satellites`: every satellite with its latest status.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::SatelliteResponse;
use crate::errors::ApiError;
use crate::models::satellite;
use crate::state::AppState;

pub async fn handle(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let satellites = list(&state).await?;
    Ok(Json(json!({ "satellites": satellites })))
}

/// Every satellite, with the fleet's latest status for each.
///
/// # Errors
/// An internal error when the database cannot be reached.
pub async fn list(state: &AppState) -> Result<Vec<SatelliteResponse>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let satellites = satellite::list(&mut connection)
        .await?
        .into_iter()
        .map(|satellite| {
            let status = state.fleet.satellite_status(satellite.id);
            SatelliteResponse::new(satellite, status)
        })
        .collect();
    Ok(satellites)
}
