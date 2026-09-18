// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/initiatives/{id}/restore`: bring a deleted initiative back, with its items.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{publish_initiative_write, publish_member_items};
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::initiative;
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

    let restored = initiative::restore(&mut connection, id, Actor::User, now).await?;
    let initiative = publish_initiative_write(&state, &mut connection, restored, now).await?;
    publish_member_items(&state, &mut connection, id, now).await?;

    Ok(Json(json!({ "initiative": initiative })))
}
