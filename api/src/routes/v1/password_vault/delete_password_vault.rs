// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/password-vault`: remove the vault. The CLI signs out and stops, its stored
//! credentials are deleted, and agents lose the tools at their next turn. The account itself is
//! untouched; revoke its API key in Bitwarden to be sure it is never used again.

use anyhow::Context;
use axum::extract::State;
use axum::http::StatusCode;

use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::password_vault;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<StatusCode, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    if !password_vault::delete(&mut connection).await? {
        return Err(ApiError::NotFound);
    }
    drop(connection);

    state.passwords.disconnect().await;
    state.events.publish(&ServerEvent::PasswordVaultDeleted);
    Ok(StatusCode::NO_CONTENT)
}
