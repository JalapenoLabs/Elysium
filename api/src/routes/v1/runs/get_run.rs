// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/runs/{id}`: one run, every step in order, and its history.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::Value;
use uuid::Uuid;

use super::detail;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::workflow_run;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let run = workflow_run::find(&mut connection, id).await?;
    let steps = workflow_run::steps_of(&mut connection, id).await?;
    Ok(Json(detail(&mut connection, run, steps).await?))
}
