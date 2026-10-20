// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/updates`: the release running, auto-update's state, and recent updates.

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
    let status = state.updates.status().await?;
    Ok(Json(json!({ "updates": status })))
}
