// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/users/{id}/revoke-sessions`: sign a person out everywhere, and disconnect every
//! MCP client they connected. Pages and clients they have open are refused from their next
//! request on (`crate::auth::sessions`, `crate::oauth::tokens`).

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use super::identity_of;
use crate::auth::{AdminUser, sessions};
use crate::errors::ApiError;
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
    sessions::end_all_sessions(&state.redis, &state.auth.kratos, identity_of(&person)?).await?;
    // Everywhere includes the MCP clients they connected. Signing out is the whole of this
    // action, so a failure of either fails the request and records nothing, for the admin to
    // retry. Disabling (`update_user`) only logs one, since the disabled row already refuses
    // everything.
    tokens::revoke_all_grants(&state.redis, &state.auth.hydra, person.id).await?;
    user::record_account_action(
        &mut connection,
        id,
        admin.id(),
        UserEventKind::SessionsRevoked,
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}
