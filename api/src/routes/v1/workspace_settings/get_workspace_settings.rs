// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/workspace-settings`: the workspace settings, for admins.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::WorkspaceSettingsResponse;
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::user;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let settings = user::workspace_settings(&mut connection).await?;
    Ok(Json(
        json!({ "settings": WorkspaceSettingsResponse::from(settings) }),
    ))
}
