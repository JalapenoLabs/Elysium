// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/timeline/{id}/history` and `POST /api/v1/timeline/{id}/history/{entryId}/undo`:
//! every change to what one of the signed-in person's records is attributed to, Elysia's and
//! theirs, oldest first, and undoing one.
//!
//! An undo is a change of its own, recorded in the history and naming the entry it reversed, so
//! it can be undone in turn. Only an attribution still holding what the entry wrote is undone:
//! one changed since answers `409` (`attribution_changed`) rather than losing that change. See
//! `crate::models::attribution`.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{HistoryEntryResponse, answer_changed};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::{attribution, timeline_record};
use crate::state::AppState;

pub async fn list(
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
    let (record, _attributions) =
        timeline_record::find_owned(&mut connection, id, current.id()).await?;
    let history: Vec<HistoryEntryResponse> = attribution::history(&mut connection, record.id)
        .await?
        .into_iter()
        .map(HistoryEntryResponse::from)
        .collect();
    Ok(Json(json!({ "history": history })))
}

pub async fn undo(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, entry_id)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let (record, _attributions) =
        timeline_record::find_owned(&mut connection, id, current.id()).await?;
    let entry = attribution::undo(&mut connection, record.id, entry_id, current.id()).await?;
    answer_changed(&state, &mut connection, id, current.id(), Some(entry)).await
}
