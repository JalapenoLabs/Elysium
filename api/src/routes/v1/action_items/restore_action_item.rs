// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/action-items/{id}/restore`: bring a deleted item back as it was.

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
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let restored = action_item::restore(&mut connection, id, Actor::User, now).await?;
    let item = publish_item_write(&state, &mut connection, restored, &[], now).await?;

    Ok(Json(json!({ "item": item })))
}
