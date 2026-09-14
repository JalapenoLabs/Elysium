// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/llms`: every credential in the order they should be tried.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::LlmResponse;
use crate::errors::ApiError;
use crate::models::llm;
use crate::state::AppState;

pub async fn handle(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let llms: Vec<LlmResponse> = llm::list(&mut connection)
        .await?
        .into_iter()
        .map(LlmResponse::from)
        .collect();

    Ok(Json(json!({ "llms": llms })))
}
