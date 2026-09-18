// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/jira-credentials/{id}/projects`: the projects the stored token can reach
//! now, marked with what the credential already allows, for the edit form.
//!
//! Nothing is cached. A listing is asked for once when the form opens, and a project added
//! on Jira should show up the first time someone looks.

use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::open;
use crate::errors::ApiError;
use crate::models::jira_credential::Allowlist;
use crate::state::AppState;

/// One project a token can reach, and whether this credential already allows it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectChoice {
    id: String,
    key: String,
    name: String,
    selected: bool,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let stored = open(&state, id).await?;
    let listing = state.jira.list_projects(&stored.site()).await?;

    let projects: Vec<ProjectChoice> = listing
        .items
        .into_iter()
        .map(|project| ProjectChoice {
            selected: match &stored.allowed.projects {
                Allowlist::All => true,
                Allowlist::Only(allowed) => allowed.iter().any(|picked| picked.id == project.id),
            },
            id: project.id,
            key: project.key,
            name: project.name,
        })
        .collect();

    Ok(Json(json!({
        "projects": projects,
        "truncated": listing.truncated,
    })))
}
