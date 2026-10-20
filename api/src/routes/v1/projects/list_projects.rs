// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/projects`: every project, alphabetically.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::ProjectResponse;
use crate::errors::ApiError;
use crate::models::project;
use crate::state::AppState;

pub async fn handle(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let projects = list(&state).await?;
    Ok(Json(json!({ "projects": projects })))
}

/// Every project, alphabetically.
///
/// # Errors
/// An internal error when the database cannot be reached.
pub async fn list(state: &AppState) -> Result<Vec<ProjectResponse>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let projects = project::list(&mut connection)
        .await?
        .into_iter()
        .map(ProjectResponse::from)
        .collect();
    Ok(projects)
}
