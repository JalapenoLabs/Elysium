// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/learning-categories/{key}`: remove one of the workspace's own categories.
//! Admins only.
//!
//! A category learnings are filed under, deleted ones included, is removed only with
//! `?moveTo=<key>`, the category they move to: each move is a revision in that learning's
//! history, so none is left without a category and each can be undone. Without it the removal
//! answers `409 category_in_use`. A built-in category answers `409` too.

use anyhow::Context;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::Utc;
use serde::Deserialize;

use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::learning::Author;
use crate::models::learning_category;
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoveQuery {
    /// The category its learnings move to.
    move_to: Option<String>,
}

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    path: Result<Path<String>, PathRejection>,
    query: Result<Query<RemoveQuery>, QueryRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(key) = path?;
    let Query(query) = query?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let author = Author {
        user_id: admin.id(),
        agent: None,
        at: Utc::now(),
    };
    let moved =
        learning_category::remove(&mut connection, &key, query.move_to.as_deref(), author).await?;

    for recorded in &moved {
        state.events.publish(&ServerEvent::LearningChanged {
            id: recorded.learning.id,
        });
    }
    state
        .events
        .publish(&ServerEvent::LearningCategoriesChanged);
    Ok(StatusCode::NO_CONTENT)
}
