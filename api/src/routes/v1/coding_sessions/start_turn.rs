// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/coding-sessions/{id}/turns`: send a prompt to the session's agent.
//!
//! The turn queues behind any the session has unfinished. Its progress arrives as
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
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::coding_session;
use crate::sessions::TurnExtras;
use crate::sessions::views::TurnView;
use crate::state::AppState;
use crate::studio::instructions::INSTRUCTIONS;

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
    current: CurrentUser,
    path: Result<Path<i64>, PathRejection>,
    body: Result<Json<StartTurnRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let turn = start_turn(&state, id, body, current.id()).await?;
    Ok((StatusCode::CREATED, Json(json!({ "turn": turn }))))
}

/// Sends `by`'s prompt to session `id`, queued behind any turn it has unfinished.
///
/// # Errors
/// The request's validation errors, and `404` for an unknown session.
pub async fn start_turn(
    state: &AppState,
    id: i64,
    body: StartTurnRequest,
    by: uuid::Uuid,
) -> Result<TurnView, ApiError> {
    body.validate()?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let session = coding_session::find(&mut connection, id).await?;
    drop(connection);

    // A Studio session reached this way still works as Studio's agents do.
    let extras = TurnExtras {
        system_prompt_append: session
            .studio_item_id
            .map(|_studio_item| INSTRUCTIONS.to_owned()),
    };
    state
        .sessions
        .start_turn(&session, body.prompt, extras, by)
        .await
}
