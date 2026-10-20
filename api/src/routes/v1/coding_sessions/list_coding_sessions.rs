// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/coding-sessions`: every session, newest first, with its thread state.
//!
//! [`find`] answers one session the same way; the MCP server reads it, and no route needs it.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::CodingSessionResponse;
use crate::errors::ApiError;
use crate::models::coding_session;
use crate::state::AppState;

pub async fn handle(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let sessions = list(&state).await?;
    Ok(Json(json!({ "sessions": sessions })))
}

/// Every session, newest first, with the fleet's latest view of its thread.
///
/// # Errors
/// An internal error when the database cannot be reached.
pub async fn list(state: &AppState) -> Result<Vec<CodingSessionResponse>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let sessions = coding_session::list(&mut connection)
        .await?
        .into_iter()
        .map(|session| {
            let thread = state.fleet.thread_status(session.id);
            CodingSessionResponse::new(session, thread)
        })
        .collect();
    Ok(sessions)
}

/// The session `id`, with the fleet's latest view of its thread.
///
/// # Errors
/// `404` for an unknown session.
pub async fn find(state: &AppState, id: i64) -> Result<CodingSessionResponse, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let session = coding_session::find(&mut connection, id).await?;
    Ok(CodingSessionResponse::new(
        session,
        state.fleet.thread_status(id),
    ))
}
