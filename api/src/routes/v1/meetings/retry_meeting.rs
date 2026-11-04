// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/meetings/{id}/retry`: try a failed meeting again from the step it failed at.
//! A meeting whose fetch failed has its run's failed step queued again, which the engine takes
//! at once; any other is picked up by the next processing pass, within a minute.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::MeetingResponse;
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
    // A meeting that has not failed answers 404 as well: there is nothing to retry.
    let retried = workflow::retry(&mut connection, &state.events, id, current.id()).await?;
    Ok(Json(json!({ "meeting": MeetingResponse::new(retried) })))
}
