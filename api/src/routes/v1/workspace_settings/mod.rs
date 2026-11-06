// Copyright © 2026 Jalapeno Labs

//! `/api/v1/workspace-settings`: settings that apply to everyone, which admins change.

mod fetch_agent_configuration;
mod get_workspace_settings;
mod update_workspace_settings;

use axum::Router;
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::agent_configuration::{self, Hold};
use crate::models::agent_configuration::Status;
use crate::models::user::WorkspaceSettings;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(get_workspace_settings::handle).patch(update_workspace_settings::handle),
        )
        .route(
            "/agent-configuration/fetch",
            post(fetch_agent_configuration::handle),
        )
}

/// The workspace settings as clients see them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSettingsResponse {
    /// Whether anyone may sign up. The first person may sign up either way.
    signup_open: bool,
    /// Whether every person must set up an authenticator app.
    require_mfa: bool,
    /// The git repository every agent's `~/.claude` is checked out from.
    agent_configuration_repository: Option<String>,
    /// The GitHub token that reads it; `null` reads it as anyone may.
    agent_configuration_github_credential_id: Option<Uuid>,
    /// How its last fetch went, and whether workers and Project Managers wait on it.
    agent_configuration: AgentConfigurationResponse,
    /// How many workers run at once; `null` follows the default the engine derives from its CPU
    /// cores, which the work queue reports.
    worker_pool_size: Option<i32>,
    updated_by: Uuid,
    updated_at: DateTime<Utc>,
}

/// The configuration repository's last fetch, as the settings page shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigurationResponse {
    /// Why workers and Project Managers wait; `null` while they may start.
    hold: Option<Hold>,
    /// The last good checkout of the repository and token the settings name, if any.
    checkout: Option<CheckoutResponse>,
}

/// A checkout of the configuration repository.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutResponse {
    commit: String,
    subject: String,
    fetched_at: DateTime<Utc>,
    /// What it held that the workspace cannot run, and so left out: `[{ kind, name, reason,
    /// detail? }]`.
    skipped: Value,
}

impl WorkspaceSettingsResponse {
    /// `settings`, with how the configuration repository's last fetch, `status`, went.
    pub fn new(settings: WorkspaceSettings, status: &Status) -> Self {
        let hold = agent_configuration::hold(&settings, status);
        // A checkout of another repository or token than the settings name is not shown.
        let checkout = match (
            &hold,
            &status.commit_sha,
            &status.commit_subject,
            status.fetched_at,
        ) {
            (None | Some(Hold::Failed { .. }), Some(commit), Some(subject), Some(fetched_at)) => {
                Some(CheckoutResponse {
                    commit: commit.clone(),
                    subject: subject.clone(),
                    fetched_at,
                    skipped: status.skipped.clone(),
                })
            }
            _otherwise => None,
        };
        Self {
            signup_open: settings.signup_open,
            require_mfa: settings.require_mfa,
            agent_configuration_repository: settings.agent_configuration_repository,
            agent_configuration_github_credential_id: settings
                .agent_configuration_github_credential_id,
            agent_configuration: AgentConfigurationResponse { hold, checkout },
            worker_pool_size: settings.worker_pool_size,
            updated_by: settings.updated_by,
            updated_at: settings.updated_at,
        }
    }
}
