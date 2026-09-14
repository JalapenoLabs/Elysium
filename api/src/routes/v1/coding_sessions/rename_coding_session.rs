// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/coding-sessions/{id}`: rename a session.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{CodingSessionResponse, validate_not_blank};
use crate::errors::ApiError;
use crate::models::coding_session;
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 200), custom(function = "validate_not_blank"))]
    title: String,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
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

    Ok(Json(json!({ "session": response })))
}
