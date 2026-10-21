// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/password-vault/collections`: every collection the account reaches, for
//! choosing the one agents may read. Nothing is typed by hand.

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
    let collections = state.passwords.collections().await.map_err(vault_error)?;
    Ok(Json(json!({ "collections": collections })))
}
