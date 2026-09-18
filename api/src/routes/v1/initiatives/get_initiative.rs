// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/initiatives/{id}`: one initiative with its progress, deleted or not.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::initiative_responses;
use crate::errors::ApiError;
use crate::models::initiative;
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
    let found = initiative::find(&mut connection, id).await?;
    let initiative = initiative_responses(&mut connection, vec![found], Utc::now())
        .await?
        .pop()
        .context("one initiative in, one response out")?;

    Ok(Json(json!({ "initiative": initiative })))
}
