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
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::validate_not_blank;
use crate::errors::ApiError;
use crate::fleet::views::TurnView;
use crate::models::coding_session;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// Large enough for a pasted stack trace or spec, well under the request body limit.
    #[validate(
        length(min = 1, max = 100_000),
        custom(function = "validate_not_blank")
    )]
    prompt: String,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
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

    Ok((
        StatusCode::CREATED,
        Json(json!({ "turn": TurnView::from(turn.queued()) })),
    ))
}
