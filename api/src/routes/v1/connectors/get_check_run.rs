// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/connectors/check-runs/{runId}`: a check run, with every check's message, for the
//! person who started it. A finished run is kept for half an hour; a stored connector's results
//! outlive it on the connector.

use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(run_id) = path?;
    let run = state
        .connectors
        .check_runs
        .get(run_id, current.id())
        .ok_or(ApiError::NotFound)?;
    Ok(Json(json!({ "run": run })))
}
