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
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let satellite = satellite::find(&mut connection, id).await?;
    let status = state.fleet.satellite_status(id);

    Ok(Json(
        json!({ "satellite": SatelliteResponse::new(satellite, status) }),
    ))
}
