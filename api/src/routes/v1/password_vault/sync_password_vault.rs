// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/password-vault/sync`: download the vault now, so an item just added in
//! Bitwarden reaches agents without waiting for the next scheduled sync.

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::vault_error;
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<Json<Value>, ApiError> {
    state.passwords.sync().await.map_err(vault_error)?;
    Ok(Json(json!({ "status": state.passwords.status() })))
}
