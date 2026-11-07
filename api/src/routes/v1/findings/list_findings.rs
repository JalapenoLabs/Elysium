// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/projects/{id}/findings`: the project's open findings, newest first, at most 200,
//! and how many are open in all, which the project's Findings tab counts.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::FindingResponse;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::{finding, project};
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(project_id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    project::find(&mut connection, project_id).await?;
    let open = finding::list_open(&mut connection, project_id).await?;
    let total = finding::count_open(&mut connection, project_id).await?;
    let findings: Vec<FindingResponse> = open.into_iter().map(FindingResponse::from).collect();
    Ok(Json(json!({ "findings": findings, "total": total })))
}
