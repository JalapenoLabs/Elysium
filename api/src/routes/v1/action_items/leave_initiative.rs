// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/action-items/{id}/initiatives/{initiative_id}`: take an item out of an
//! initiative. The initiative's burnup keeps the time it was a member. An item not in it
//! is answered unchanged.

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
    let Path((id, initiative_id)) = path?;
    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let left =
        action_item::leave_initiative(&mut connection, id, initiative_id, Actor::User, now).await?;
    let item = publish_item_write(&state, &mut connection, left, &[initiative_id], now).await?;

    Ok(Json(json!({ "item": item })))
}
