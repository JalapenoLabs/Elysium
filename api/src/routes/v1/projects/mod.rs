// Copyright © 2026 Jalapeno Labs

//! `/api/v1/projects`: what coding sessions are grouped under.

mod create_project;
mod delete_cover;
mod delete_project;
mod get_cover;
mod list_projects;
mod update_project;
mod upload_cover;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, patch};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;
use validator::ValidationError;

use crate::images::MAX_UPLOAD_BYTES;
use crate::models::project::{Project, ProjectCoverFit};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_projects::handle).post(create_project::handle))
        .route(
            "/{id}",
            patch(update_project::handle).delete(delete_project::handle),
        )
        .route(
            "/{id}/cover",
            get(get_cover::handle)
                .put(upload_cover::handle)
                .delete(delete_cover::handle)
                // Uploads outgrow the API-wide body limit; the image module refuses anything
                // over its own limit before decoding.
                .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)),
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
    /// When the cover last changed, or `None` without one. Clients add it to the cover's
    /// URL, so a changed cover is fetched afresh and an unchanged one comes from cache.
    cover_updated_at: Option<DateTime<Utc>>,
    cover_fit: ProjectCoverFit,
}

impl From<Project> for ProjectResponse {
    fn from(project: Project) -> Self {
        Self {
            id: project.id,
            name: project.name,
            description: project.description,
            created_at: project.created_at,
            updated_at: project.updated_at,
            cover_updated_at: project.cover_image_updated_at,
            cover_fit: project.cover_fit,
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
