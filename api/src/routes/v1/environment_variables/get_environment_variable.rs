// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/environment-variables/{id}`: one environment variable.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::EnvironmentVariableResponse;
use crate::errors::ApiError;
use crate::models::environment_variable;
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

    let variable = environment_variable::find(&mut connection, id).await?;

    Ok(Json(json!({
        "variable": EnvironmentVariableResponse::new(variable, &state.cipher)?,
    })))
}
