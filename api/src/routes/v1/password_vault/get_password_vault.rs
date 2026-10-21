// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/password-vault`: the vault, if one is set up, and what its CLI is doing.

use anyhow::Context;
use axum::Json;
use axum::extract::State;

use super::{PasswordVaultResponse, VaultBody};
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::password_vault;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<Json<VaultBody>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let vault = password_vault::find(&mut connection).await?;
    Ok(Json(VaultBody {
        vault: vault.as_ref().map(PasswordVaultResponse::from),
        status: state.passwords.status(),
    }))
}
