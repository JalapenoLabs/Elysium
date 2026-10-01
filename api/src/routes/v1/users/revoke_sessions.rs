// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/users/{id}/revoke-sessions`: sign a person out everywhere, and disconnect every
//! MCP client they connected. Pages and clients they have open stop working within the session
//! and token caches' lifetimes (`crate::auth::sessions`, `crate::mcp::bearer`).

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
    state
        .auth
        .kratos
        .revoke_sessions(identity_of(&person)?)
        .await?;
    // Everywhere includes the MCP clients they connected. Signing out is the whole of this
    // action, so a failure of either fails the request and records nothing, for the admin to
    // retry. Disabling (`update_user`) only logs one, since the disabled row already refuses
    // everything.
    state.auth.hydra.revoke_consent(person.id, None).await?;
    user::record_account_action(
        &mut connection,
        id,
        admin.id(),
        UserEventKind::SessionsRevoked,
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}
