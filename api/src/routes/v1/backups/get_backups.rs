// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/backups`: where the databases are backed up, when next, and recent attempts.

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<Json<Value>, ApiError> {
    let status = state.backups.status().await?;
    Ok(Json(json!({ "backups": status })))
}
