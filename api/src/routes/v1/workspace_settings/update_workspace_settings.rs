// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/workspace-settings`: open or close sign-up, require an authenticator app of
//! everyone, or name the repository agents' `~/.claude` comes from.
//!
//! A changed repository is fetched into the workspace in the background; a fetch that fails
//! is logged, and the next turn's preparation reports it to whoever started the turn.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{Level, event};
use url::Url;

use super::WorkspaceSettingsResponse;
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::user::{self, WorkspaceSettingsChanges};
use crate::realtime::ServerEvent;
use crate::state::AppState;

/// The longest repository URL kept, as the column's check allows.
const MAX_REPOSITORY_URL_LENGTH: usize = 2_048;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[serde(default)]
    signup_open: Option<bool>,
    #[serde(default)]
    require_mfa: Option<bool>,
    /// Absent leaves it, `null` clears it, and a URL sets it.
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[expect(
        clippy::option_option,
        reason = "absent, null, and a URL are three distinct requests"
    )]
    agent_configuration_repository: Option<Option<String>>,
}

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body?;
    let repository = body
        .agent_configuration_repository
        .map(|repository| repository.map(|url| validate_repository(&url)).transpose())
        .transpose()?;
    let repository_changes = repository.is_some();
    let changes = WorkspaceSettingsChanges {
        signup_open: body.signup_open,
        require_mfa: body.require_mfa,
        agent_configuration_repository: repository,
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let settings = user::update_workspace_settings(&mut connection, changes, admin.id()).await?;
    drop(connection);

    if repository_changes {
        let runner = state.runner.clone();
        tokio::spawn(async move {
            if let Err(error) = runner.refresh_configuration().await {
                event!(
                    name: "runner.configuration.refresh_failed",
                    Level::ERROR,
                    error.message = %format!("{error:#}"),
                    "the new configuration repository could not be fetched",
                );
            }
        });
    }

    let response = WorkspaceSettingsResponse::from(settings);
    state
        .events
        .publish(&ServerEvent::WorkspaceSettingsUpdated(response.clone()));
    Ok(Json(json!({ "settings": response })))
}

/// Accepts an https URL with a path and no credentials, the shape the column allows. A token
/// in the URL would be stored in the clear and shown to every admin; the default GitHub token
/// is used instead.
fn validate_repository(raw: &str) -> Result<String, ApiError> {
    let trimmed = raw.trim();
    let refuse =
        |reason: &str| ApiError::BadRequest(format!("agentConfigurationRepository {reason}"));
    if trimmed.len() > MAX_REPOSITORY_URL_LENGTH {
        return Err(refuse("is longer than 2048 characters"));
    }
    let url = Url::parse(trimmed).map_err(|_parse_error| refuse("is not a URL"))?;
    if url.scheme() != "https" {
        return Err(refuse("must be an https URL"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(refuse(
            "must not carry credentials; a github.com repository is read with the default GitHub token",
        ));
    }
    if url.host_str().is_none() || url.path().trim_matches('/').is_empty() {
        return Err(refuse("must name a repository"));
    }
    Ok(url.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repository_is_an_https_url_without_credentials() {
        assert_eq!(
            validate_repository(" https://github.com/someone/dotclaude.git ").expect("accepted"),
            "https://github.com/someone/dotclaude.git"
        );
        for refused in [
            "http://github.com/someone/dotclaude",
            "https://x-access-token:ghp_x@github.com/someone/dotclaude",
            "https://github.com/",
            "git@github.com:someone/dotclaude.git",
        ] {
            assert!(
                validate_repository(refused).is_err(),
                "{refused} is refused"
            );
        }
    }
}
