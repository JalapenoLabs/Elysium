// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/action-items/{id}/initiatives/{initiative_id}`: put an item in an
//! initiative, adding it to the initiative's scope. An item already in it is answered
//! unchanged; a deleted initiative answers `409`.

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

    let joined =
        action_item::join_initiative(&mut connection, id, initiative_id, Actor::User, now).await?;
    let item = publish_item_write(&state.events, &mut connection, joined, &[], now).await?;

    Ok(Json(json!({ "item": item })))
}
