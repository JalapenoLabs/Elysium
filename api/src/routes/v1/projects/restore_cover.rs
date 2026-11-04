// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/projects/{id}/cover/restore`: show a removed cover again, as Undo does.
//!
//! A project with no removed cover answers as it is, so a cover uploaded since the removal
//! stays.

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
    let updated = project::restore_cover(&mut connection, id).await?;

    let response = ProjectResponse::from(updated);
    let body = json!({ "project": &response });
    state
        .events
        .publish(&ServerEvent::ProjectUpserted(response));
    Ok(Json(body))
}
