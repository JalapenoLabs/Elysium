// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/action-items/{id}/links/{link_id}`: unlink an item. The linked thing is
//! left as it is in its provider, and writes the link still owed are dropped with it. When
//! the primary goes, the oldest link left becomes primary and the owner follows it.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use uuid::Uuid;

use super::{publish_item_links, publish_item_write};
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::action_item_link;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path((id, link_id)) = path?;
    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let unlinked = action_item_link::remove(&mut connection, id, link_id, Actor::User, now).await?;
    state.events.publish(&ServerEvent::ActionItemLinkDeleted {
        id: link_id,
        action_item_id: id,
    });
    publish_item_write(&state.events, &mut connection, unlinked.item, &[], now).await?;
    publish_item_links(&state.events, &mut connection, id).await?;

    Ok(StatusCode::NO_CONTENT)
}
