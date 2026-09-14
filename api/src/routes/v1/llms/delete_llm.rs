// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/llms/{id}`: remove a credential and its sealed token.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::llm;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    llm::delete(&mut connection, id).await?;
    state.events.publish(&ServerEvent::LlmDeleted { id });

    Ok(StatusCode::NO_CONTENT)
}
