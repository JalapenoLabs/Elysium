// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/action-items/{id}/comments/{comment_id}`: rewrite one of the user's
//! comments. Someone else's comment answers `409`.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::create_comment::RequestBody;
use super::{CommentResponse, publish_history};
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::action_item_comment;
use crate::models::action_item_event::Recorded;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, comment_id)) = path?;
    let Json(body) = body?;
    body.validate()?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let Recorded { record, history } = action_item_comment::update(
        &mut connection,
        id,
        comment_id,
        body.body,
        Actor::User,
        Utc::now(),
    )
    .await?;
    drop(connection);

    let comment = CommentResponse::from(record);
    if !history.is_empty() {
        publish_history(&state, history);
        state
            .events
            .publish(&ServerEvent::ActionItemCommentUpserted(comment.clone()));
    }

    Ok(Json(json!({ "comment": comment })))
}
