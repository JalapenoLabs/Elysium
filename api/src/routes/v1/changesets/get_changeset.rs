// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/changesets/{id}`: one changeset with its operations, in any state.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::ChangesetResponse;
use crate::errors::ApiError;
use crate::models::changeset;
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
    let found = changeset::find(&mut connection, id).await?;
    Ok(Json(json!({ "changeset": ChangesetResponse::from(found) })))
}
