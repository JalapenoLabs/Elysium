// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/action-items/{id}/projects/{project_id}`: take an item out of a
//! project. An item not in it is answered unchanged.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::publish_item_write;
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::action_item;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, project_id)) = path?;
    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let removed =
        action_item::remove_project(&mut connection, id, project_id, Actor::User, now).await?;
    let item = publish_item_write(&state.events, &mut connection, removed, &[], now).await?;

    Ok(Json(json!({ "item": item })))
}
