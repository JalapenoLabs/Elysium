// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/work-batches/{id}/cancel`: withdraw a batch no worker has started on, with an
//! optional `reason` its Project Manager is told. Answers it as `GET` does. `409`
//! (`work_batch_moved_on`) once a worker started on it or it ended.

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
use crate::state::AppState;
use crate::worker_pool::actions;

/// What a batch cancelled without a reason records, for its Project Manager to read.
const NO_REASON: &str = "Cancelled from the work queue, without a reason given.";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[serde(default)]
    reason: Option<String>,
}

pub async fn handle(
    State(state): State<AppState>,
    MemberUser(member): MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let reason = body
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .unwrap_or(NO_REASON);
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let batch =
        actions::cancel(&mut connection, &state.events, id, None, reason, member.id).await?;
    Ok(Json(with_history(&mut connection, batch).await?))
}
