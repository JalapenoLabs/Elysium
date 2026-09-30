// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/coding-sessions/{id}/events`: the session's kept event history.
//!
//! Elysium records every event its session watcher receives (`crate::fleet`), so history is
//! read from Postgres and outlives the thread. Events after the response arrive live on the
//! event stream; clients merge the two on `sequence`. Only the latest [`HISTORY_LIMIT`] are
//! returned.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};

use crate::errors::ApiError;
use crate::fleet::views::SessionEvent;
use crate::models::{coding_session, session_event};
use crate::state::AppState;

/// Most events one response carries. A long session's streamed output runs to
/// thousands of small events; the conversation view needs the recent end of it.
const HISTORY_LIMIT: usize = 5_000;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<i64>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    // Answers 404 for a session that does not exist, rather than an empty history.
    coding_session::find(&mut connection, id).await?;
    let (events, truncated) = session_event::latest(&mut connection, id, HISTORY_LIMIT).await?;

    let events: Vec<SessionEvent> = events
        .into_iter()
        .map(|thread_event| SessionEvent::new(id, thread_event))
        .collect();
    Ok(Json(json!({ "events": events, "truncated": truncated })))
}
