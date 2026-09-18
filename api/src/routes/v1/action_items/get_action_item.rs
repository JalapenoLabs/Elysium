// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/action-items/{id}`: one item, deleted or not, so its history can be shown
//! and it can be restored.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::item_responses;
use crate::errors::ApiError;
use crate::models::action_item;
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
    let item = action_item::find(&mut connection, id).await?;
    let item = item_responses(&mut connection, vec![item])
        .await?
        .pop()
        .context("one item in, one response out")?;

    Ok(Json(json!({ "item": item })))
}
