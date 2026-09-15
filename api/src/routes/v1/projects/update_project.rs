// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/projects/{id}`: rename or redescribe a project.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{ProjectResponse, validate_not_blank};
use crate::errors::ApiError;
use crate::models::project::{self, ProjectChanges};
use crate::realtime::ServerEvent;
use crate::state::AppState;

/// Absent fields stay as they are.
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 120), custom(function = "validate_not_blank"))]
    name: Option<String>,
    #[validate(length(max = 2000))]
    description: Option<String>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    body.validate()?;

    let changes = ProjectChanges {
        name: body.name,
        description: body.description,
    };
    if changes.is_empty() {
        return Err(ApiError::BadRequest(
            "request body contains no fields to update".to_owned(),
        ));
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let project = project::update(&mut connection, id, &changes).await?;

    state
        .events
        .publish(&ServerEvent::ProjectUpserted(ProjectResponse::from(
            project.clone(),
        )));

    Ok(Json(json!({ "project": ProjectResponse::from(project) })))
}
