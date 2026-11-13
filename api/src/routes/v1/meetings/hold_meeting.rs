// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/meetings/{id}/hold` and `DELETE`: a meeting's owner holds it for their approval
//! of its corrected transcript before it is acted on, whatever its source's setting, or stops
//! holding it. Either is taken only before the meeting is corrected; after, `409`, since approving
//! the copy is what lets it go on. `404` for a meeting they may not read, `403` `owner_only` for
//! anyone but its owner.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{deleted_refusal, merged_refusal, open, response};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::access::Need;
use crate::meetings::correct::{self, HoldError};
use crate::models::meeting::Reader;
use crate::state::AppState;

pub async fn hold(
    state: State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    set(state, current, path, true).await
}

pub async fn release(
    state: State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    set(state, current, path, false).await
}

async fn set(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
    held: bool,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    open(&mut connection, id, &current, Need::Own).await?;
    let changed = correct::hold(&mut connection, &state.events, id, current.id(), held)
        .await
        .map_err(|error| match error {
            HoldError::NotFound => ApiError::NotFound,
            HoldError::Merged => merged_refusal(),
            HoldError::Deleted => deleted_refusal(),
            HoldError::Corrected => ApiError::CodedConflict {
                code: "meeting_corrected",
                message: "The meeting's transcript is corrected already; approve it instead.",
            },
            HoldError::Database(database) => database.into(),
        })?;
    let answered = response(&mut connection, changed, Reader::of(&current.0)).await?;
    Ok(Json(json!({ "meeting": answered })))
}
