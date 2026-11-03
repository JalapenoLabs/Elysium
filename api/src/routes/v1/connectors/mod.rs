// Copyright © 2026 Jalapeno Labs

//! `/api/v1/connectors`: the hub. Every connector the signed-in person sees, what each kind
//! is, their checks, and their recent syncs. See `crate::connectors` and `docs/connectors.md`.
//!
//! A workspace kind's connectors (Jira, GitHub) are everyone's, like their accounts; a personal
//! kind's (mail, meetings) answer their creator alone, and are `404` to anyone else. No route
//! matches on the kind: `crate::connectors::kinds` does.

mod check_draft;
mod create_connector;
mod delete_connector;
mod get_check_run;
mod get_connector;
mod list_activity;
mod list_connectors;
mod list_kinds;
mod list_options;
mod run_checks;
mod sync_connector;
mod update_connector;

use std::collections::HashMap;

use anyhow::Context;
use axum::Router;
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::connectors::accounts::{self, AccountSummary};
use crate::connectors::kinds::{self, is_visible};
use crate::connectors::{ConnectorKind, Scope, SignInState};
use crate::errors::ApiError;
use crate::models::connector::{self, Connector};
use crate::models::connector_check::{self, CheckResult};
use crate::models::connector_run::{self, ConnectorRun};
use crate::models::jira_credential;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(list_connectors::handle).post(create_connector::handle),
        )
        .route("/kinds", get(list_kinds::handle))
        .route("/activity", get(list_activity::handle))
        .route("/options", post(list_options::handle))
        .route("/checks", post(check_draft::handle))
        .route("/check-runs/{run_id}", get(get_check_run::handle))
        .route(
            "/{id}",
            get(get_connector::handle)
                .patch(update_connector::handle)
                .delete(delete_connector::handle),
        )
        .route("/{id}/checks", post(run_checks::handle))
        .route("/{id}/sync", post(sync_connector::handle))
}

/// The longest API key accepted. Fireflies' are well under a hundred characters.
const API_KEY_MAX_BYTES: usize = 512;

/// An API key from a request body, checked as it is parsed so a validation report never
/// carries it. Surrounding whitespace, which copying often brings, is dropped.
#[derive(Debug)]
pub struct ApiKey(pub SecretString);

impl<'de> Deserialize<'de> for ApiKey {
    fn deserialize<Source: Deserializer<'de>>(deserializer: Source) -> Result<Self, Source::Error> {
        let key = String::deserialize(deserializer)?;
        let key = key.trim();
        if key.is_empty() {
            return Err(serde::de::Error::custom("apiKey must not be blank"));
        }
        if key.len() > API_KEY_MAX_BYTES {
            return Err(serde::de::Error::custom("apiKey exceeds 512 characters"));
        }
        Ok(Self(SecretString::from(key)))
    }
}

/// Where a connector stands, as the hub shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Active,
    Syncing,
    /// Its last read failed, or its account is asking to be signed in again.
    Error,
    Paused,
}

/// A connector's status: paused above all, then a sync under way, then whether its last read
/// left an error.
pub fn status(
    is_active: bool,
    is_syncing: bool,
    last_error: Option<&str>,
    sign_in: &str,
) -> Status {
    if !is_active {
        return Status::Paused;
    }
    if is_syncing {
        return Status::Syncing;
    }
    if last_error.is_some() || sign_in == SignInState::SignedOut.as_str() {
        return Status::Error;
    }
    Status::Active
}

/// One check's last result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResultView {
    check_id: String,
    state: String,
    count: Option<i64>,
    reason: Option<String>,
    message: Option<String>,
    blocked_by: Option<String>,
    ran_at: DateTime<Utc>,
}

impl From<CheckResult> for CheckResultView {
    fn from(result: CheckResult) -> Self {
        Self {
            check_id: result.check_id,
            state: result.state,
            count: result.count,
            reason: result.reason,
            message: result.message,
            blocked_by: result.blocked_by,
            ran_at: result.ran_at,
        }
    }
}

/// A connector as the hub shows it. Never carries a secret.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorView {
    id: Uuid,
    kind: String,
    scope: Scope,
    account: Option<AccountSummary>,
    status: Status,
    is_active: bool,
    /// `signed_in`, `signed_out`, or `unknown`, as last read.
    sign_in_state: String,
    last_sync_at: Option<DateTime<Utc>>,
    /// Why the last read failed, or `null` when it succeeded.
    last_error: Option<String>,
    /// How many things it has recorded: timeline records, or linked issues for Jira and GitHub.
    records: i64,
    /// What it reads, in its kind's own terms.
    content: Value,
    storage_location_id: Option<Uuid>,
    checks: Vec<CheckResultView>,
    /// Its newest sync worth showing, when it had one.
    last_run: Option<RunView>,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

/// One sync, as the activity feed shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    id: Uuid,
    connector_id: Uuid,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    /// `succeeded` or `failed`.
    outcome: String,
    records: i32,
    error: Option<String>,
}

impl From<ConnectorRun> for RunView {
    fn from(run: ConnectorRun) -> Self {
        Self {
            id: run.id,
            connector_id: run.connector_id,
            started_at: run.started_at,
            finished_at: run.finished_at,
            outcome: run.outcome,
            records: run.records,
            error: run.error,
        }
    }
}

/// The views of `connectors`, in their order, each with its account, counts, checks, and
/// last sync.
///
/// # Errors
/// Propagates any database error.
pub async fn views(
    state: &AppState,
    connectors: Vec<Connector>,
) -> Result<Vec<ConnectorView>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let ids: Vec<Uuid> = connectors.iter().map(|connector| connector.id).collect();
    let summaries = accounts::summaries(&mut connection, &connectors).await?;
    let records = accounts::record_counts(&mut connection, &connectors).await?;
    let passes = accounts::watcher_passes(&mut connection, &connectors).await?;
    let mut checks: HashMap<Uuid, Vec<CheckResultView>> = HashMap::new();
    for result in connector_check::list_for(&mut connection, &ids).await? {
        checks
            .entry(result.connector_id)
            .or_default()
            .push(result.into());
    }
    let mut last_runs: HashMap<Uuid, RunView> = connector_run::latest_each(&mut connection, &ids)
        .await?
        .into_iter()
        .map(|run| (run.connector_id, run.into()))
        .collect();

    let mut views: Vec<ConnectorView> = Vec::with_capacity(connectors.len());
    for stored in connectors {
        let kind = kinds::kind_of(&stored)?;
        let pass = passes.get(&stored.id);
        let last_error = stored
            .last_error
            .clone()
            .or_else(|| pass.and_then(|pass| pass.last_error.clone()));
        let last_sync_at = stored
            .last_polled_at
            .or_else(|| pass.map(|pass| pass.updated_at));
        let content = match (kind, stored.jira_credential_id) {
            // A Jira connector's content is its account's allowlist. Another kind reading
            // through the same Atlassian account, such as Confluence, keeps its own.
            (ConnectorKind::Jira, Some(credential_id)) => {
                let credential = jira_credential::find(&mut connection, credential_id).await?;
                let allowed = jira_credential::allowed_of(&mut connection, &credential).await?;
                json!({ "projects": allowed.projects, "boards": allowed.boards })
            }
            _ => stored.content.clone(),
        };
        views.push(ConnectorView {
            id: stored.id,
            kind: stored.kind.clone(),
            scope: kinds::definition(kind).scope,
            account: summaries.get(&stored.account().id()).cloned(),
            status: status(
                stored.is_active,
                state.connectors.activity.is_syncing(stored.id),
                last_error.as_deref(),
                &stored.sign_in_state,
            ),
            is_active: stored.is_active,
            sign_in_state: stored.sign_in_state.clone(),
            last_sync_at,
            last_error,
            records: records.get(&stored.id).copied().unwrap_or(0),
            content,
            storage_location_id: stored.storage_location_id,
            checks: checks.remove(&stored.id).unwrap_or_default(),
            last_run: last_runs.remove(&stored.id),
            created_by: stored.created_by,
            created_at: stored.created_at,
        });
    }
    Ok(views)
}

/// The connector `id`, when `person` may see it.
///
/// # Errors
/// [`ApiError::NotFound`] when it does not exist or is someone else's personal connector.
pub async fn visible(state: &AppState, id: Uuid, person: Uuid) -> Result<Connector, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let found = connector::find(&mut connection, id).await?;
    if !is_visible(&found, person) {
        return Err(ApiError::NotFound);
    }
    Ok(found)
}

/// The view of one connector.
///
/// # Errors
/// As [`views`].
pub async fn view(state: &AppState, stored: Connector) -> Result<ConnectorView, ApiError> {
    views(state, vec![stored])
        .await?
        .pop()
        .ok_or(ApiError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paused_connector_is_paused_whatever_else_is_true() {
        assert_eq!(
            status(false, true, Some("refused"), "signed_out"),
            Status::Paused
        );
    }

    #[test]
    fn a_sync_under_way_shows_over_an_old_error() {
        assert_eq!(
            status(true, true, Some("refused"), "signed_in"),
            Status::Syncing
        );
    }

    #[test]
    fn an_error_or_a_sign_in_prompt_is_an_error() {
        assert_eq!(
            status(true, false, Some("refused"), "signed_in"),
            Status::Error
        );
        assert_eq!(status(true, false, None, "signed_out"), Status::Error);
    }

    #[test]
    fn a_connector_reading_cleanly_is_active() {
        assert_eq!(status(true, false, None, "signed_in"), Status::Active);
        assert_eq!(status(true, false, None, "unknown"), Status::Active);
    }

    #[test]
    fn an_api_key_drops_surrounding_whitespace_and_refuses_blanks() {
        use secrecy::ExposeSecret;
        let key: ApiKey = serde_json::from_value(json!("  key-1  ")).expect("parses");
        assert_eq!(key.0.expose_secret(), "key-1");
        serde_json::from_value::<ApiKey>(json!("   ")).expect_err("blank");
    }
}
