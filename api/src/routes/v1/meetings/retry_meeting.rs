// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/meetings/{id}/retry`: try a failed meeting again from the step it failed at.
//! Its run's failed step is queued again with a fresh set of attempts, as the person who asked,
//! and the engine takes it at once, just as Retry on the run's page does.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::response;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::workflow;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    // A meeting whose run has not failed answers 404 as well: there is nothing to retry.
    let retried = workflow::retry(&mut connection, &state.events, id, current.id()).await?;
    let answered = response(&mut connection, retried).await?;
    Ok(Json(json!({ "meeting": answered })))
}
