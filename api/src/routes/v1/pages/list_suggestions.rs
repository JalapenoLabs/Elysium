// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/pages/{id}/suggestions`: the edits agents suggested to a page that wait for a
//! person, oldest first, each as its changes and whether each still merges with the page.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::pending_suggestions;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let suggestions = pending_suggestions(&mut connection, id).await?;
    Ok(Json(json!({ "suggestions": suggestions })))
}
