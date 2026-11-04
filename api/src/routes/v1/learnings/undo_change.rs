// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/learnings/{id}/history/{entryId}/undo`: reverse one change to a learning.
//!
//! The reversal is a change of its own, recorded in the history and naming the entry it undid,
//! so nothing is ever lost and an undo can itself be undone. A revision is undone field by
//! field, keeping any field changed again since; see `crate::models::learning::undo`.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{announce, author};
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::learning;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, entry_id)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let recorded = learning::undo(&mut connection, id, entry_id, author(&member, None)).await?;

    Ok(Json(json!({ "learning": announce(&state, recorded) })))
}
