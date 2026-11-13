// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/connectors/{id}/meeting-count`: how many of the signed-in person's meetings the
//! connector listed, `{ "count": 3 }`, which its removal offers to delete with it
//! (`delete_connector`). They are the meetings whose first capture it read, neither deleted nor
//! merged into another (`meeting::listed_by`), the same ones the removal deletes. A connector
//! that provides no meetings listed none.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::visible;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::meeting;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    visible(&state, id, current.id()).await?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let listed = meeting::listed_by(&mut connection, id, current.id()).await?;
    Ok(Json(json!({ "count": listed.len() })))
}
