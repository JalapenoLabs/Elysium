// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/jira-credentials/{id}/issues/{key}`: one issue, with its description and
//! comments.

use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{allowed_issue, open};
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, String)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, key)) = path?;
    let stored = open(&state, id).await?;

    // Checked from the key and again against the project Jira reports, so an issue moved to
    // another project cannot be read through a stale key.
    let issue = allowed_issue(&state.jira, &stored, &key).await?;

    Ok(Json(json!({ "issue": issue })))
}
