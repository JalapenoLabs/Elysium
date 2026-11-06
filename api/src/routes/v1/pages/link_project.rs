// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/pages/{id}/projects/{project_id}`: link a page to a project, which then lists it
//! among its docs. A link already there is answered unchanged. A link is no version: it is recorded
//! in the page's history alone, and changes who can read the page in no way.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{PageResponse, answer, author};
use crate::auth::MemberUser;
use crate::database::Pool;
use crate::errors::ApiError;
use crate::models::agent::Author;
use crate::models::page_project;
use crate::realtime::EventBus;
use crate::state::AppState;

/// A page and a project: what the link tools take, where a route has both in its path.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageProjectArguments {
    /// The page, from `page_list`.
    pub id: Uuid,
    /// The project's id.
    pub project_id: Uuid,
}

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, project_id)) = path?;
    let page = link(
        &state.database,
        &state.events,
        author(&member),
        id,
        project_id,
    )
    .await?;
    Ok(Json(json!({ "page": page })))
}

/// Links page `id` to project `project_id` as `author`: what this route and an agent's tool both do.
///
/// # Errors
/// `404` for an unknown page or project, and `409 page_deleted` for a deleted page.
pub async fn link(
    database: &Pool,
    events: &EventBus,
    author: Author<'_>,
    id: Uuid,
    project_id: Uuid,
) -> Result<PageResponse, ApiError> {
    let mut connection = database
        .get()
        .await
        .context("no database connection available")?;
    let recorded = page_project::link(&mut connection, id, project_id, author).await?;
    answer(events, &mut connection, recorded).await
}
