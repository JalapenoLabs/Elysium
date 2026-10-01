// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/users/{id}`: refuse a pending sign-up. The account and its Kratos
//! identity are deleted, so the address can sign up again. Approved people are never
//! deleted, since rows they created name them; they are disabled instead.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use diesel_async::AsyncConnection;
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

    let kratos = &state.auth.kratos;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    // Kratos deletes the identity inside the transaction: if it refuses, the account stays,
    // and the admin can reject it again.
    connection
        .transaction(async |connection| {
            let rejected = user::reject(connection, id, admin.id()).await?;
            kratos.delete_identity(identity_of(&rejected)?).await?;
            Ok::<_, ApiError>(())
        })
        .await?;

    state.events.publish(&ServerEvent::UserDeleted { id });
    Ok(StatusCode::NO_CONTENT)
}
