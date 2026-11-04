// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/projects/{id}/project-manager`: whether the project's Project Manager writes
//! its changes directly, or proposes each for a person to approve, the default. It applies from
//! the Project Manager's next change.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::ProjectManagerResponse;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::project::{self, ProjectChanges};
use crate::project_managers;
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    writes_directly: bool,
}

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(project_id) = path?;
    let Json(body) = body?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let changes = ProjectChanges {
        manager_writes_directly: Some(body.writes_directly),
        ..ProjectChanges::default()
    };
    project::update(&mut connection, project_id, &changes).await?;
    state
        .events
        .publish(&ServerEvent::ProjectManagerChanged { project_id });

    let snapshot = project_managers::snapshot(&mut connection, project_id).await?;
    Ok(Json(json!({
        "projectManager": ProjectManagerResponse::from(snapshot),
    })))
}
