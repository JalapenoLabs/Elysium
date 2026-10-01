// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/users/{id}/revoke-sessions`: sign a person out everywhere. Pages they have
//! open stop working within the session cache's lifetime (`crate::auth::sessions`).

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
    user::record_account_action(
        &mut connection,
        id,
        admin.id(),
        UserEventKind::SessionsRevoked,
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}
