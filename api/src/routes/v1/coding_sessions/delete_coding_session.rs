// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/coding-sessions/{id}`: stop the session's turns, remove its directory,
//! and forget it.
//!
//! Deleting takes the session's kept history with it. A directory that will not go is logged
//! and left in the workspace; it never blocks the delete.
//!
//! A Studio item's sessions are its conversation, so they go only with their item.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::errors::ApiError;
use crate::models::coding_session;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let session = coding_session::find(&mut connection, id).await?;
    drop(connection);
    if session.studio_item_id.is_some() {
        return Err(ApiError::Conflict(
            "the session belongs to a Studio item; delete the item instead",
        ));
    }

    state.sessions.remove(id).await;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    coding_session::delete(&mut connection, id).await?;

    state.events.publish(&ServerEvent::SessionDeleted { id });

    Ok(StatusCode::NO_CONTENT)
}
