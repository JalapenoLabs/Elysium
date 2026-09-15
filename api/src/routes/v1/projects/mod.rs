// Copyright © 2026 Jalapeno Labs

//! `/api/v1/projects`: what coding sessions are grouped under.

mod create_project;
mod delete_project;
mod list_projects;
mod update_project;

use axum::Router;
use axum::routing::{get, patch};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;
use validator::ValidationError;

use crate::models::project::Project;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_projects::handle).post(create_project::handle))
        .route(
            "/{id}",
            patch(update_project::handle).delete(delete_project::handle),
        )
}

/// A project as clients see it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectResponse {
    id: Uuid,
    name: String,
    description: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Project> for ProjectResponse {
    fn from(project: Project) -> Self {
        Self {
            id: project.id,
            name: project.name,
            description: project.description,
            created_at: project.created_at,
            updated_at: project.updated_at,
        }
    }
}

/// Rejects names that are only whitespace; `length` alone would accept `"   "`.
fn validate_not_blank(value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::new("blank").with_message("must not be blank".into()));
    }
    Ok(())
}
