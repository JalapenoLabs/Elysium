// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/users/{id}/recovery-link`: a one-time link that signs the person in and
//! opens the page to set a new password. The admin hands it over; it is never stored.

use std::time::Duration;

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::identity_of;
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::user::{self, UserEventKind};
use crate::state::AppState;

/// How long an admin's recovery link works. Matches the self-service link's lifespan in
/// `kratos/kratos.yml`.
const RECOVERY_LINK_LIFESPAN: Duration = Duration::from_mins(15);

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let person = user::find(&mut connection, id).await?;
    let link = state
        .auth
        .kratos
        .recovery_link(identity_of(&person)?, RECOVERY_LINK_LIFESPAN)
        .await?;
    user::record_account_action(
        &mut connection,
        id,
        admin.id(),
        UserEventKind::RecoveryLinkCreated,
    )
    .await?;

    Ok(Json(json!({
        "recoveryLink": link.recovery_link,
        "expiresAt": link.expires_at,
    })))
}
