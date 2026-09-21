// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/action-items/{id}`: hide an item everywhere until it is restored.
//!
//! Deleting is soft. The item keeps its history, comments, and memberships, and stops
//! counting toward any initiative's progress.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use uuid::Uuid;

use super::publish_item_write;
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::action_item;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let deleted = action_item::soft_delete(&mut connection, id, Actor::User, now).await?;
    publish_item_write(&state.events, &mut connection, deleted, &[], now).await?;

    Ok(StatusCode::NO_CONTENT)
}
