// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/jira-credentials/{id}`: one Jira credential.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::JiraCredentialResponse;
use crate::errors::ApiError;
use crate::models::jira_credential;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let credential = jira_credential::find(&mut connection, id).await?;
    let allowed = jira_credential::allowed_of(&mut connection, &credential).await?;

    Ok(Json(
        json!({ "credential": JiraCredentialResponse::new(credential, allowed) }),
    ))
}
