// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/projects/{id}/project-manager/tracker`: stop naming a tracker, so the
//! project's Project Manager files where most of its items' issues already are.

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
use crate::models::{project, project_issue_tracker};
use crate::project_managers;
use crate::realtime::ServerEvent;
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
    if project_issue_tracker::clear(&mut connection, project_id).await? {
        state
            .events
            .publish(&ServerEvent::ProjectManagerChanged { project_id });
    }

    let snapshot =
        project_managers::snapshot(&mut connection, project_id, Reader::of(&member.0)).await?;
    Ok(Json(json!({
        "projectManager": ProjectManagerResponse::from(snapshot),
    })))
}
