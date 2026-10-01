// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/users/{id}/reset-mfa`: remove a person's authenticator app and lookup codes,
//! for someone who lost both, and sign them out, MCP clients included, so their next sign-in
//! needs only a password or passkey. They can set a new authenticator up afterwards.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use super::identity_of;
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::user::{self, UserEventKind};
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
    kratos.delete_credentials(identity, "lookup_secret").await?;
    kratos.revoke_sessions(identity).await?;
    // Part of the reset itself, so a failure fails the request, as in `revoke_sessions`.
    state.auth.hydra.revoke_consent(person.id, None).await?;
    user::record_account_action(&mut connection, id, admin.id(), UserEventKind::MfaReset).await?;

    Ok(StatusCode::NO_CONTENT)
}
