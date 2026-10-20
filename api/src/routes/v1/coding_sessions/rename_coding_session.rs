// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/coding-sessions/{id}`: rename a session.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use validator::Validate;

use super::{CodingSessionResponse, validate_not_blank};
use crate::errors::ApiError;
use crate::models::coding_session;
use crate::realtime::ServerEvent;
use crate::state::AppState;

/// A session's new title.
#[derive(Debug, Deserialize, Validate, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameCodingSessionRequest {
    /// What the session is called.
    #[validate(length(min = 1, max = 200), custom(function = "validate_not_blank"))]
    pub title: String,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<i64>, PathRejection>,
    body: Result<Json<RenameCodingSessionRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let session = rename(&state, id, body).await?;
    Ok(Json(json!({ "session": session })))
}

/// Renames the session `id` and announces it.
///
/// # Errors
/// The request's validation errors, or `404` for an unknown session.
pub async fn rename(
    state: &AppState,
    id: i64,
    body: RenameCodingSessionRequest,
) -> Result<CodingSessionResponse, ApiError> {
    body.validate()?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let session = coding_session::rename(&mut connection, id, &body.title).await?;

    let response = CodingSessionResponse::new(session, state.fleet.thread_status(id));
    state
        .events
        .publish(&ServerEvent::SessionUpserted(response.clone()));
    Ok(response)
}
