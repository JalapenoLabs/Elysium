// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/action-items/{id}/comments/{comment_id}`: remove one of the user's
//! comments. Its body stays in the item's history. Someone else's comment answers `409`.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use uuid::Uuid;

use super::publish_history;
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::action_item_comment;
use crate::models::action_item_event::Recorded;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path((id, comment_id)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let Recorded { history, .. } =
        action_item_comment::delete(&mut connection, id, comment_id, Actor::User, Utc::now())
            .await?;
    drop(connection);

    publish_history(&state.events, history);
    state
        .events
        .publish(&ServerEvent::ActionItemCommentDeleted {
            id: comment_id,
            action_item_id: id,
        });

    Ok(StatusCode::NO_CONTENT)
}
