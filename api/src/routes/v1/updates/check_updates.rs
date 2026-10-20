// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/updates/check`: asks Docker Hub for a newer release now, rather than at the
//! next hourly check. A failed check is part of the status, not an error response.

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
    state.updates.check().await;
    let status = state.updates.status().await?;
    Ok(Json(json!({ "updates": status })))
}
