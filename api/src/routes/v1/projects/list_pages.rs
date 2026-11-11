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
use crate::models::{page_label, page_project, page_suggestion, project};
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
    fn new(linked: ProjectPage, labels: Vec<String>, pending_suggestions: i64) -> Self {
        Self {
            page: PageSummaryResponse::new(linked.page, labels, pending_suggestions),
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
    let linked = page_project::pages_of(&mut connection, project_id).await?;
    let page_ids: Vec<Uuid> = linked.iter().map(|linked| linked.page.id).collect();
    let mut waiting = page_suggestion::pending_counts(&mut connection, Some(&page_ids)).await?;
    let pages: Vec<ProjectPageResponse> = linked
        .into_iter()
        .map(|linked| {
            let labels = labels.remove(&linked.page.id).unwrap_or_default();
            let pending_suggestions = waiting.remove(&linked.page.id).unwrap_or_default();
            ProjectPageResponse::new(linked, labels, pending_suggestions)
        })
        .collect();
    Ok(Json(json!({ "pages": pages })))
}
