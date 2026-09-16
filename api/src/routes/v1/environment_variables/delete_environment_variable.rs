// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/environment-variables/{id}`: stop passing a variable to new threads.
//!
//! Threads already running keep the environment they were created with.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::environment_variable;
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

    environment_variable::delete(&mut connection, id).await?;

    state
        .events
        .publish(&ServerEvent::EnvironmentVariableDeleted { id });

    Ok(StatusCode::NO_CONTENT)
}
