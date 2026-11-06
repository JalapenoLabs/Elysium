// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/projects/{id}/pages`: the knowledge base pages linked to a project, its docs, newest
//! published first. Pages are the knowledge base's, so this answers the company's members alone,
//! whichever project it names: being on a project gives a guest none of its pages.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::page_project::ProjectPage;
use crate::models::{page_label, page_project, project};
use crate::routes::v1::pages::PageSummaryResponse;
use crate::state::AppState;

/// A page as a project lists it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectPageResponse {
    #[serde(flatten)]
    page: PageSummaryResponse,
    /// Who published its newest version, and when.
    published_by: Uuid,
    published_at: DateTime<Utc>,
    /// Who linked it to the project, and when.
    linked_by: Uuid,
    linked_at: DateTime<Utc>,
}

impl ProjectPageResponse {
    fn new(linked: ProjectPage, labels: Vec<String>) -> Self {
        Self {
            page: PageSummaryResponse::new(linked.page, labels),
            published_by: linked.published_by,
            published_at: linked.published_at,
            linked_by: linked.linked_by,
            linked_at: linked.linked_at,
        }
    }
}

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
    let mut labels = page_label::by_page(&mut connection).await?;
    let pages: Vec<ProjectPageResponse> = page_project::pages_of(&mut connection, project_id)
        .await?
        .into_iter()
        .map(|linked| {
            let labels = labels.remove(&linked.page.id).unwrap_or_default();
            ProjectPageResponse::new(linked, labels)
        })
        .collect();
    Ok(Json(json!({ "pages": pages })))
}
