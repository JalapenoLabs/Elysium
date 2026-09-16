// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/environment-variables`: every environment variable, by key.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::EnvironmentVariableResponse;
use crate::errors::ApiError;
use crate::models::environment_variable;
use crate::state::AppState;

pub async fn handle(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let variables = environment_variable::list(&mut connection)
        .await?
        .into_iter()
        .map(|variable| EnvironmentVariableResponse::new(variable, &state.cipher))
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(Json(json!({ "variables": variables })))
}
