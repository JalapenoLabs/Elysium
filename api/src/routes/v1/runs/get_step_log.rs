// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/runs/{id}/steps/{step_id}/log`: what a step wrote as it ran, every attempt's,
//! the newest 64 KiB of it. Read on its own, as the run page opens a step, since a run's steps
//! may have written far more than the page shows at once.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::step_job;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((run_id, step_id)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let log = step_job::log_of(&mut connection, run_id, step_id).await?;
    Ok(Json(json!({ "log": log })))
}
