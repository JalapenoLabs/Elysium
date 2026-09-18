// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/jira-credentials/{id}/issues/{key}/transitions`: the moves an issue can
//! make right now, for whoever the stored token is.

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
    allowlist::issue_project(&stored.allowed.projects, &stored.credential.name, &key)?;

    let transitions = state.jira.transitions(&stored.site(), &key).await?;

    Ok(Json(json!({ "transitions": transitions })))
}
