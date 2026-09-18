// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/action-items/{id}/accept`, `/resolve`, `/dismiss`, and `/reopen`: move an
//! item to another state.
//!
//! A transition the item's state does not allow answers `409`; the rules are
//! `crate::action_items::Transition`.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::publish_item_write;
use crate::action_items::{Actor, Transition};
use crate::errors::ApiError;
use crate::models::action_item;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    transition: Transition,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let moved = action_item::transition(&mut connection, id, transition, Actor::User, now).await?;
    let item = publish_item_write(&state, &mut connection, moved, &[], now).await?;

    Ok(Json(json!({ "item": item })))
}
