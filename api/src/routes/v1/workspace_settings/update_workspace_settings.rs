// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/workspace-settings`: open or close sign-up, require an authenticator app of
//! everyone, name the repository agents' `~/.claude` comes from and the GitHub token that reads
//! it, or size the worker pool.
//!
//! A changed repository or token is fetched into the workspace by the engine at once
//! (`crate::agent_configuration::keeper`), which is the check: the settings page shows the
//! commit it fetched and when, or why it could not, as the engine announces it.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;
use uuid::Uuid;

use super::WorkspaceSettingsResponse;
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::user::{self, WorkspaceSettingsChanges};
use crate::models::{agent_configuration, github_credential};
use crate::realtime::{ServerEvent, Signal};
use crate::runner::configuration::TOKEN_HOST;
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
    /// Absent leaves it, `null` reads the repository without a token, and an id chooses that
    /// GitHub token, for a github.com repository alone.
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[expect(
        clippy::option_option,
        reason = "absent, null, and an id are three distinct requests"
    )]
    agent_configuration_github_credential_id: Option<Option<Uuid>>,
    /// Absent leaves it, `null` follows the default, and a whole number of at least one sets it.
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[expect(
        clippy::option_option,
        reason = "absent, null, and a size are three distinct requests"
    )]
    worker_pool_size: Option<Option<i32>>,
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
    let configuration_changes =
        repository.is_some() || body.agent_configuration_github_credential_id.is_some();
    if body.worker_pool_size.flatten().is_some_and(|size| size < 1) {
        return Err(ApiError::BadRequest(
            "workerPoolSize must be at least one worker".to_owned(),
        ));
    }
    let pool_changes = body.worker_pool_size.is_some();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    if configuration_changes {
        let current = user::workspace_settings(&mut connection).await?;
        check_token_choice(
            &mut connection,
            repository
                .clone()
                .unwrap_or(current.agent_configuration_repository),
            body.agent_configuration_github_credential_id
                .unwrap_or(current.agent_configuration_github_credential_id),
        )
        .await?;
    }
    let changes = WorkspaceSettingsChanges {
        signup_open: body.signup_open,
        require_mfa: body.require_mfa,
        agent_configuration_repository: repository,
        agent_configuration_github_credential_id: body.agent_configuration_github_credential_id,
        worker_pool_size: body.worker_pool_size,
        ..WorkspaceSettingsChanges::default()
    };
    let settings = user::update_workspace_settings(&mut connection, changes, admin.id()).await?;
    let status = agent_configuration::find(&mut connection).await?;
    drop(connection);

    if configuration_changes {
        // The engine fetches the repository with its token into the workspace, and records how
        // it went; until then workers and Project Managers wait.
        state.events.signal(&Signal::AgentConfiguration);
    }
    if pool_changes {
        // The pool is the engine's; it fills slots it gained, or takes back those it lost.
        state.events.signal(&Signal::WorkerPool);
        state.events.publish(&ServerEvent::WorkerPoolChanged);
    }

    let response = WorkspaceSettingsResponse::new(settings, &status);
    state
        .events
        .publish(&ServerEvent::WorkspaceSettingsUpdated(response.clone()));
    Ok(Json(json!({ "settings": response })))
}

/// Accepts an https URL with a path and no credentials, the shape the column allows. A token
/// in the URL would be stored in the clear and shown to every admin; the token chosen beside it
/// is sealed instead.
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
    // A query or fragment is no part of a repository, and could carry a token into the
    // settings, every batch, and every fetch's error, where it would be shown.
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(refuse(
            "must not carry credentials, a query, or a fragment; choose the GitHub token that \
             reads it instead",
        ));
    }
    if url.host_str().is_none() || url.path().trim_matches('/').is_empty() {
        return Err(refuse("must name a repository"));
    }
    Ok(url.into())
}

/// Refuses a token for a repository off github.com, where a GitHub token must never be sent,
/// and an id that names no GitHub token.
async fn check_token_choice(
    connection: &mut diesel_async::AsyncPgConnection,
    repository: Option<String>,
    github_credential_id: Option<Uuid>,
) -> Result<(), ApiError> {
    let Some(id) = github_credential_id else {
        return Ok(());
    };
    // With no repository the token waits for one; it is read only for github.com.
    let off_github = repository.as_deref().is_some_and(|repository| {
        !Url::parse(repository).is_ok_and(|url| url.host_str() == Some(TOKEN_HOST))
    });
    if off_github {
        return Err(ApiError::BadRequest(
            "agentConfigurationGithubCredentialId is only for a repository on github.com"
                .to_owned(),
        ));
    }
    match github_credential::find(connection, id).await {
        Ok(_credential) => Ok(()),
        Err(diesel::result::Error::NotFound) => Err(ApiError::BadRequest(
            "agentConfigurationGithubCredentialId names no GitHub token".to_owned(),
        )),
        Err(error) => Err(error.into()),
    }
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
            "https://github.com/someone/dotclaude.git?token=ghp_x",
            "https://github.com/someone/dotclaude.git#ghp_x",
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
