// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/action-items/{id}/history/{entry_id}/undo`: undo a change to an item's projects
//! or its lead, as the person asking. The undo is an entry of its own naming the one it reversed,
//! so it can be undone in turn. A change made since answers `409` rather than being overwritten.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::publish_item_write;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::action_item_attribution;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, entry_id)) = path?;
    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let undone =
        action_item_attribution::undo(&mut connection, id, entry_id, current.actor(), now).await?;
    let item = publish_item_write(&state.events, &mut connection, undone, &[], now).await?;

    Ok(Json(json!({ "item": item })))
}
