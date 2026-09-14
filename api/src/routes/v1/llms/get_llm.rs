// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/llms/{id}`: one credential.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::LlmResponse;
use crate::errors::ApiError;
use crate::models::llm;
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

    let llm = llm::find(&mut connection, id).await?;

    Ok(Json(json!({ "llm": LlmResponse::from(llm) })))
}
