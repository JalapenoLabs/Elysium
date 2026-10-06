// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/coding-sessions/{id}/turns`: send a prompt to the session's thread.
//!
//! The satellite queues the turn behind any that are running. Its progress arrives as
//! `session.event`s on the event stream, not in this response.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use validator::Validate;

use super::validate_not_blank;
use crate::errors::ApiError;
use crate::fleet::views::TurnView;
use crate::models::coding_session;
use crate::state::AppState;

/// A prompt for a session's agent.
#[derive(Debug, Deserialize, Validate, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartTurnRequest {
    /// What to ask the agent. Large enough for a pasted stack trace or spec, well under the
    /// request body limit.
    #[validate(
        length(min = 1, max = 100_000),
        custom(function = "validate_not_blank")
    )]
    pub prompt: String,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<i64>, PathRejection>,
    body: Result<Json<StartTurnRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let turn = start_turn(&state, id, body).await?;
    Ok((StatusCode::CREATED, Json(json!({ "turn": turn }))))
}

/// Sends a prompt to the session `id`'s thread, which queues it behind any turn running.
///
/// # Errors
/// The request's validation errors, `404` for an unknown session, or the satellite's refusal.
pub async fn start_turn(
    state: &AppState,
    id: i64,
    body: StartTurnRequest,
) -> Result<TurnView, ApiError> {
    body.validate()?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let session = coding_session::find(&mut connection, id).await?;
    drop(connection);

    let client = state.fleet.client(session.satellite_id).await?;
    let handle = client.threads().attach(session.thread_id.as_str()).await?;
    let turn = handle.start_turn(body.prompt).await?;
    Ok(TurnView::from(turn.queued()))
}
