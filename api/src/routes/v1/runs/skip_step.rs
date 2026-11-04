// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/runs/{id}/steps/{step_id}/skip`: skip a step that has not run, failed, or is
//! held. The step after it receives the input this one would have, and a run this one stopped
//! or held goes on. `409` while the step runs, or once it succeeded or was skipped.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use super::acted;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::step_job;
use crate::models::workflow_run::Actor;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    MemberUser(member): MemberUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((run_id, step_id)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let actor = Actor {
        user_id: member.id,
        at: Utc::now(),
    };
    let settled = step_job::skip(&mut connection, run_id, step_id, actor).await?;
    Ok(Json(acted(&state, &mut connection, settled).await?))
}
