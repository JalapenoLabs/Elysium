// Copyright © 2026 Jalapeno Labs

//! `/api/v1/workspace-settings`: settings that apply to everyone, which admins change.

mod get_workspace_settings;
mod update_workspace_settings;

use axum::Router;
use axum::routing::get;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::models::user::WorkspaceSettings;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/",
        get(get_workspace_settings::handle).patch(update_workspace_settings::handle),
    )
}

/// The workspace settings as clients see them.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSettingsResponse {
    /// Whether anyone may sign up. The first person may sign up either way.
    signup_open: bool,
    /// Whether every person must set up an authenticator app.
    require_mfa: bool,
    updated_by: Uuid,
    updated_at: DateTime<Utc>,
}

impl From<WorkspaceSettings> for WorkspaceSettingsResponse {
    fn from(settings: WorkspaceSettings) -> Self {
        Self {
            signup_open: settings.signup_open,
            require_mfa: settings.require_mfa,
            updated_by: settings.updated_by,
            updated_at: settings.updated_at,
        }
    }
}
