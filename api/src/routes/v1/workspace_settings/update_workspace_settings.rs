// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/workspace-settings`: open or close sign-up, or require an authenticator app
//! of everyone.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use serde::Deserialize;
use serde_json::{Value, json};

use super::WorkspaceSettingsResponse;
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::user::{self, WorkspaceSettingsChanges};
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[serde(default)]
    signup_open: Option<bool>,
    #[serde(default)]
    require_mfa: Option<bool>,
}

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body?;
    let changes = WorkspaceSettingsChanges {
        signup_open: body.signup_open,
        require_mfa: body.require_mfa,
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let settings = user::update_workspace_settings(&mut connection, changes, admin.id()).await?;

    state.events.publish(&ServerEvent::WorkspaceSettingsUpdated(
        WorkspaceSettingsResponse::from(settings),
    ));
    Ok(Json(
        json!({ "settings": WorkspaceSettingsResponse::from(settings) }),
    ))
}
