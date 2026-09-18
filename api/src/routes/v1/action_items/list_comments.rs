// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/action-items/{id}/comments`: an item's comments, oldest first.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::CommentResponse;
use crate::errors::ApiError;
use crate::models::{action_item, action_item_comment};
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
    action_item::find(&mut connection, id).await?;
    let comments: Vec<CommentResponse> = action_item_comment::list(&mut connection, id)
        .await?
        .into_iter()
        .map(CommentResponse::from)
        .collect();

    Ok(Json(json!({ "comments": comments })))
}
