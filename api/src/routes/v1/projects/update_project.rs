// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/projects/{id}`: rename or redescribe a project, or change how it picks its
//! GitHub token.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{ProjectGithub, ProjectResponse, validate_not_blank};
use crate::errors::ApiError;
use crate::models::project::{self, GithubAccess, ProjectChanges, ProjectCoverFit};
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
    cover_fit: Option<ProjectCoverFit>,
    /// Replaces the project's GitHub choice: `specific` names a token, and the other
    /// accesses name none.
    github: Option<ProjectGithub>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    body.validate()?;

    if let Some(github) = body.github {
        let names_a_token = github.credential_id.is_some();
        let needs_a_token = github.access == GithubAccess::Specific;
        if names_a_token != needs_a_token {
            return Err(ApiError::BadRequest(
                "github.credentialId is required for specific access and refused otherwise"
                    .to_owned(),
            ));
        }
    }

    let changes = ProjectChanges {
        name: body.name,
        description: body.description,
        cover_fit: body.cover_fit,
        github_access: body.github.map(|github| github.access),
        github_credential_id: body.github.map(|github| github.credential_id),
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
    let project = project::update(&mut connection, id, &changes)
        .await
        .map_err(|error| match error {
            DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, _) => {
                ApiError::BadRequest(
                    "github.credentialId names a token that does not exist".to_owned(),
                )
            }
            other => other.into(),
        })?;

    state
        .events
        .publish(&ServerEvent::ProjectUpserted(ProjectResponse::from(
            project.clone(),
        )));

    Ok(Json(json!({ "project": ProjectResponse::from(project) })))
}
