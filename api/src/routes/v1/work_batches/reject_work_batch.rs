// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/work-batches/{id}/reject`: decline a proposed batch with a `reason`, which is
//! required: its Project Manager is woken with it, and it is the feedback the rejection leaves.
//! Answers it as `GET` does. `409` (`work_batch_moved_on`) once it is no longer proposed.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use super::with_history;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::meeting::Reader;
use crate::state::AppState;
use crate::worker_pool::actions;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    reason: String,
}

pub async fn handle(
    State(state): State<AppState>,
    person: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let batch = actions::reject(&mut connection, &state.events, id, &body.reason, &person).await?;
    Ok(Json(
        with_history(&mut connection, batch, Reader::of(&person.0)).await?,
    ))
}
