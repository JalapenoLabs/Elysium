// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/pages/{id}/projects/{project_id}`: take a page off a project, keeping both. A
//! link that is not there is answered unchanged. The change is recorded in the page's history.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{PageResponse, answer, author};
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::agent::Author;
use crate::models::page_project;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, project_id)) = path?;
    let page = unlink(&state, author(&member), id, project_id).await?;
    Ok(Json(json!({ "page": page })))
}

/// Removes page `id`'s link to project `project_id` as `author`: what this route and an agent's tool
/// both do.
///
/// # Errors
/// `404` for an unknown page or project, and `409 page_deleted` for a deleted page.
pub async fn unlink(
    state: &AppState,
    author: Author<'_>,
    id: Uuid,
    project_id: Uuid,
) -> Result<PageResponse, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let recorded = page_project::unlink(&mut connection, id, project_id, author).await?;
    answer(state, &mut connection, recorded).await
}
