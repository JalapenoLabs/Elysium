// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/jira-credentials/{id}/issues/{key}`: one issue, with its description and
//! comments.

use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{allowlist, open};
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, String)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, key)) = path?;
    let stored = open(&state, id).await?;

    // Checked before the call from the key, and again below against the project Jira
    // reports, so an issue moved to another project cannot be read through a stale key.
    allowlist::issue_project(&stored.allowed.projects, &stored.credential.name, &key)?;
    let issue = state.jira.issue(&stored.site(), &key).await?;
    allowlist::ensure_allowed(
        &stored.allowed.projects,
        &stored.credential.name,
        &issue.project_key,
    )?;

    Ok(Json(json!({ "issue": issue })))
}
