// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/users/{id}/reset-mfa`: remove a person's authenticator app and lookup codes,
//! for someone who lost both, and sign them out, MCP clients included, so their next sign-in
//! needs only a password or passkey. They can set a new authenticator up afterwards; the old
//! one's nickname goes with it.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use super::identity_of;
use crate::auth::{AdminUser, sessions};
use crate::errors::ApiError;
use crate::models::authenticator_nickname;
use crate::models::user::{self, UserEventKind};
use crate::oauth::tokens;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let person = user::find(&mut connection, id).await?;
    let identity = identity_of(&person)?;

    let kratos = &state.auth.kratos;
    kratos.delete_credentials(identity, "totp").await?;
    authenticator_nickname::clear(&mut connection, person.id).await?;
    kratos.delete_credentials(identity, "lookup_secret").await?;
    sessions::end_all_sessions(&state.redis, kratos, identity).await?;
    // Part of the reset itself, so a failure fails the request, as in `revoke_sessions`.
    tokens::revoke_all_grants(&state.redis, &state.auth.hydra, person.id).await?;
    user::record_account_action(&mut connection, id, admin.id(), UserEventKind::MfaReset).await?;

    Ok(StatusCode::NO_CONTENT)
}
