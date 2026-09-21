// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/jira-credentials/{id}/projects/{key}/done-transition`: which `done` status
//! resolving an item moves the project's issues into.
//!
//! Answers the project's `done` statuses, read from Jira now, the choice stored for it, and
//! whether it needs one: a project with exactly one `done` status uses it without asking.
//! The project must be one the credential may touch. See `docs/jira.md`.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{DoneTransitionChoice, open, project_key_of};
use crate::action_items::links::jira::done_statuses;
use crate::errors::ApiError;
use crate::models::jira_done_transition;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, String)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, project_key)) = path?;
    let project_key = project_key_of(&project_key)?;
    let stored = open(&state, id).await?;
    let statuses = done_statuses(&state.jira, &stored, project_key).await?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let chosen = jira_done_transition::find(&mut connection, id, project_key).await?;
    let statuses: Vec<Value> = statuses
        .into_iter()
        .map(|status| json!({ "id": status.id, "name": status.name }))
        .collect();

    Ok(Json(json!({
        "projectKey": project_key,
        "needsChoice": statuses.len() > 1,
        "statuses": statuses,
        "chosen": chosen.map(DoneTransitionChoice::from),
    })))
}
