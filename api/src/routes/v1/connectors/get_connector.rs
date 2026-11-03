// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/connectors/{id}`: one connector, which `connector.changed` names.

use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{view, visible};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let stored = visible(&state, id, current.id()).await?;
    Ok(Json(json!({ "connector": view(&state, stored).await? })))
}
