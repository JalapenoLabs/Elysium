// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/users/{id}`: change an approved person's role, or disable and re-enable
//! them. The workspace always keeps one active admin.
//!
//! Disabling stops the Kratos identity from signing in and signs it out everywhere; the
//! person's own requests are refused at once, since every request reads their row. The row and
//! Kratos change together: Kratos is told inside the transaction, which rolls back if it
//! refuses.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use diesel_async::AsyncConnection;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;

use super::{UserResponse, identity_of};
use crate::auth::AdminUser;
use crate::auth::kratos::IdentityState;
use crate::errors::ApiError;
use crate::models::user::{self, PersonRole};
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[serde(default)]
    role: Option<PersonRole>,
    #[serde(default)]
    disabled: Option<bool>,
}

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    if body.role.is_none() && body.disabled.is_none() {
        return Err(ApiError::BadRequest(
            "name a role, or whether the account is disabled".to_owned(),
        ));
    }

    let kratos = &state.auth.kratos;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    // One transaction for the whole change, and Kratos last inside it: if Kratos refuses, the
    // role and status changes roll back with it, so Elysium never shows an account as enabled
    // while Kratos still refuses its sign-in.
    let changed = connection
        .transaction(async |connection| {
            let mut changed = user::find(connection, id).await?;
            if let Some(role) = body.role {
                changed = user::change_role(connection, id, role, admin.id()).await?;
            }
            if let Some(disabled) = body.disabled {
                changed = user::set_disabled(connection, id, disabled, admin.id()).await?;
                let kratos_state = if disabled {
                    IdentityState::Inactive
                } else {
                    IdentityState::Active
                };
                kratos
                    .set_state(identity_of(&changed)?, kratos_state)
                    .await?;
            }
            Ok::<_, ApiError>(changed)
        })
        .await?;
    drop(connection);

    // An inactive identity's sessions already fail, and the disabled row refuses every request,
    // the MCP server's included, so this is thoroughness rather than the lock: a failure is
    // logged, not answered.
    if body.disabled == Some(true) {
        if let Err(error) = kratos.revoke_sessions(identity_of(&changed)?).await {
            event!(
                name: "auth.sessions.revoke_failure",
                Level::WARN,
                error.message = %error,
                "disabled a person but could not sign their sessions out",
            );
        }
        if let Err(error) = state.auth.hydra.revoke_consent(changed.id, None).await {
            event!(
                name: "oauth.grants.revoke_failure",
                Level::WARN,
                error.message = %error,
                "disabled a person but could not disconnect their MCP clients",
            );
        }
    }

    state
        .events
        .publish(&ServerEvent::UserUpserted(UserResponse::from(
            changed.clone(),
        )));
    Ok(Json(json!({ "user": UserResponse::from(changed) })))
}
