// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/jira-credentials/{id}/projects/{key}/done-transition`: forget the done
//! transition chosen for a project. A project with one `done` status uses it again; one
//! with several waits for a new choice before it moves an issue.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use super::allowlist::ensure_allowed;
use super::open;
use crate::errors::ApiError;
use crate::models::jira_done_transition;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, String)>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path((id, project_key)) = path?;
    let stored = open(&state, id).await?;
    ensure_allowed(
        &stored.allowed.projects,
        &stored.credential.name,
        &project_key,
    )?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    jira_done_transition::forget(&mut connection, id, &project_key).await?;
    Ok(StatusCode::NO_CONTENT)
}
