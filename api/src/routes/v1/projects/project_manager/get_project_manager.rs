// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/projects/{id}/project-manager`: the project's Project Manager, its latest turns,
//! and its batches. A project nothing has woken yet answers it idle, with none.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::ProjectManagerResponse;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::meeting::Reader;
use crate::models::project;
use crate::project_managers;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(project_id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    project::find(&mut connection, project_id).await?;
    let snapshot =
        project_managers::snapshot(&mut connection, project_id, Reader::of(&member.0)).await?;
    Ok(Json(json!({
        "projectManager": ProjectManagerResponse::from(snapshot),
    })))
}
