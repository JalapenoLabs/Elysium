// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/users/{id}`: refuse a pending sign-up. The account and its Kratos
//! identity are deleted, so the address can sign up again. Approved people are never
//! deleted, since rows they created name them; they are disabled instead.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use super::identity_of;
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::user;
use crate::realtime::ServerEvent;
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
    let rejected = user::reject(&mut connection, id, admin.id()).await?;
    drop(connection);

    // The row is gone either way; an identity Kratos failed to delete can sign in again and
    // lands as a new pending sign-up, which an admin can reject again.
    state
        .auth
        .kratos
        .delete_identity(identity_of(&rejected)?)
        .await?;

    state.events.publish(&ServerEvent::UserDeleted { id });
    Ok(StatusCode::NO_CONTENT)
}
