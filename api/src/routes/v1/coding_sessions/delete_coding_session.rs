// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/coding-sessions/{id}`: destroy the thread and forget the session.
//!
//! A thread that already expired or was destroyed, or whose satellite was deleted, does not
//! block the delete. Any other satellite failure does, so a session is never forgotten while
//! its thread still runs. Deleting takes the session's kept history with it.
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

    // A session whose satellite was deleted has no thread left to destroy.
    if let Some(satellite_id) = session.satellite_id {
        let client = state.fleet.client(satellite_id).await?;
        let destroyed = match client.threads().attach(session.thread_id.as_str()).await {
            Ok(handle) => handle.destroy().await.map(drop),
            Err(error) => Err(error),
        };
        match destroyed {
            Ok(()) => {}
            Err(error) if error.is_gone() || error.is_not_found() => {}
            Err(error) => return Err(error.into()),
        }
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    coding_session::delete(&mut connection, id).await?;

    state.fleet.forget_session(id);
    state.events.publish(&ServerEvent::SessionDeleted { id });

    Ok(StatusCode::NO_CONTENT)
}
