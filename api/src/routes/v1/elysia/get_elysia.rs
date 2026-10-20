// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/elysia`: Elysia at a glance: her color, what holds her back, her open
//! questions, and the work she has taken on.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::ElysiaResponse;
use crate::elysia;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let snapshot = elysia::snapshot(&mut connection).await?;
    Ok(Json(json!({ "elysia": ElysiaResponse::from(snapshot) })))
}
