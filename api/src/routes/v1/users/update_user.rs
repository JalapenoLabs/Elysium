// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/users/{id}`: change an approved person's role, or disable and re-enable
//! them. The workspace always keeps one active admin.
//!
//! Disabling stops the Kratos identity from signing in and signs it out everywhere; the
//! person's own requests are refused at once, since every request reads their row.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
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

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let mut changed = user::find(&mut connection, id).await?;
    if let Some(role) = body.role {
        changed = user::change_role(&mut connection, id, role, admin.id()).await?;
    }
    if let Some(disabled) = body.disabled {
        changed = user::set_disabled(&mut connection, id, disabled, admin.id()).await?;
        drop(connection);

        let identity = identity_of(&changed)?;
        let kratos_state = if disabled {
            IdentityState::Inactive
        } else {
            IdentityState::Active
        };
        state.auth.kratos.set_state(identity, kratos_state).await?;
        if disabled {
            state.auth.kratos.revoke_sessions(identity).await?;
        }
    }

    state
        .events
        .publish(&ServerEvent::UserUpserted(UserResponse::from(
            changed.clone(),
        )));
    Ok(Json(json!({ "user": UserResponse::from(changed) })))
}
