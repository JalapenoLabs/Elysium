// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/action-items/{id}/links/{link_id}/primary`: make one of an item's links its
//! primary, where comments are posted and whose assignee owns the item. The owner follows
//! that link's assignee at once. Answers every link of the item.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{publish_item_links, publish_item_write};
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::action_item_link;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, link_id)) = path?;
    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let promoted =
        action_item_link::make_primary(&mut connection, id, link_id, Actor::User, now).await?;
    publish_item_write(&state.events, &mut connection, promoted.item, &[], now).await?;
    let links = publish_item_links(&state.events, &mut connection, id).await?;

    Ok(Json(json!({ "links": links })))
}
