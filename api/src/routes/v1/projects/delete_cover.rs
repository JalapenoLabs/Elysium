// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/projects/{id}/cover`: remove a project's cover image.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::ProjectResponse;
use crate::errors::ApiError;
use crate::models::project;
use crate::realtime::ServerEvent;
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
    let updated = project::set_cover(&mut connection, id, None).await?;

    let response = ProjectResponse::from(updated);
    let body = json!({ "project": &response });
    state
        .events
        .publish(&ServerEvent::ProjectUpserted(response));
    Ok(Json(body))
}
