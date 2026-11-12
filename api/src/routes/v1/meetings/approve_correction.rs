// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/meetings/{id}/correction/approve`: the signed-in person approves their
//! meeting's corrected transcript. It resolves the inbox item that asked for the review and,
//! when the correction holds the meeting's run, lets the run go on, saying the copy was
//! approved. Approving twice answers the first approval. `404` for a meeting that is not
//! theirs or not corrected yet.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{merged_refusal, response};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::correct::{self, ApproveError};
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
    let approved = correct::approve(&mut connection, &state.events, id, current.id())
        .await
        .map_err(|error| match error {
            ApproveError::NotFound => ApiError::NotFound,
            ApproveError::Merged => merged_refusal(),
            ApproveError::Run(refused) => refused.into(),
            ApproveError::Database(database) => database.into(),
        })?;
    // Answered after the approval committed, so it shows the run going on.
    let answered = response(&mut connection, approved).await?;
    Ok(Json(json!({ "meeting": answered })))
}
