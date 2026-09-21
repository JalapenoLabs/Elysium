// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/action-items/{id}/links/{link_id}/writes/{write_id}`: give up on a
//! provider write that has not landed, such as closing an issue whose project waits for a
//! done transition. The item's own change stands; the item's history records the cancel.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use uuid::Uuid;

use super::{publish_item_links, publish_item_write};
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::action_item_link_write;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, Uuid, Uuid)>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path((id, link_id, write_id)) = path?;
    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let cancelled =
        action_item_link_write::cancel(&mut connection, id, link_id, write_id, Actor::User, now)
            .await?;
    publish_item_write(&state.events, &mut connection, cancelled, &[], now).await?;
    publish_item_links(&state.events, &mut connection, id).await?;

    Ok(StatusCode::NO_CONTENT)
}
