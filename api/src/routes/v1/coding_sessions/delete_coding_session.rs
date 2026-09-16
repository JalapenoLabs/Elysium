// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/coding-sessions/{id}`: destroy the thread and forget the session.
//!
//! A thread that already expired or was destroyed does not block the delete. Any other
//! satellite failure does, so a session is never forgotten while its thread still runs.

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

    let client = state.fleet.client(session.satellite_id).await?;
    let destroyed = match client.threads().attach(session.thread_id.as_str()).await {
        Ok(handle) => handle.destroy().await.map(drop),
        Err(error) => Err(error),
    };
    match destroyed {
        Ok(()) => {}
        Err(error) if error.is_gone() || error.is_not_found() => {}
        Err(error) => return Err(error.into()),
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
