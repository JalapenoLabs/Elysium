// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/issue-reports/preview`: exactly what a report would post, and the tokens it
//! could be filed with. Nothing reaches GitHub.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::HeaderMap;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{Report, environment};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::issue_reports::{
    self, REPORT_LIMIT, REPORT_WINDOW, REPOSITORY_NAME, REPOSITORY_OWNER, Readiness,
};
use crate::models::github_credential::{self, GithubTokenKind};
use crate::models::issue_report::{self, Filing};
use crate::state::AppState;

/// One of the reporter's own tokens, with whether it can file the report.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CredentialChoice {
    id: Uuid,
    name: String,
    kind: GithubTokenKind,
    /// The account the report would be filed as.
    login: String,
    is_default: bool,
    readiness: Readiness,
}

/// The last time the reporter reported this problem.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviousReport {
    issue_number: i64,
    issue_url: String,
    url: String,
    filed_as: Filing,
    reported_at: DateTime<Utc>,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    headers: HeaderMap,
    body: Result<Json<Report>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(report) = body?;
    let note = report.note()?;
    let draft = issue_reports::prepare(
        &state.database,
        &state.connectors.check_runs,
        current.id(),
        &report.subject,
        note,
        &environment(&state, &headers),
    )
    .await?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let now = Utc::now();
    let credentials = github_credential::list_created_by(&mut connection, current.id()).await?;
    let suggested = issue_reports::suggested_credential(&credentials, now);
    let previous = issue_report::latest_by(&mut connection, current.id(), &draft.fingerprint)
        .await?
        .map(|previous| PreviousReport {
            issue_number: previous.issue_number,
            issue_url: previous.issue_url,
            url: previous.url,
            filed_as: previous.filed_as,
            reported_at: previous.created_at,
        });
    let window = chrono::Duration::from_std(REPORT_WINDOW).context("the window fits")?;
    let (made, _oldest) =
        issue_report::made_since(&mut connection, current.id(), now - window).await?;
    drop(connection);

    let choices: Vec<CredentialChoice> = credentials
        .into_iter()
        .map(|credential| CredentialChoice {
            readiness: issue_reports::readiness(&credential, now),
            id: credential.id,
            name: credential.name,
            kind: credential.kind,
            login: credential.login,
            is_default: credential.is_default,
        })
        .collect();

    Ok(Json(json!({
        "draft": {
            "title": draft.title,
            "body": draft.body,
            "fingerprint": draft.fingerprint,
        },
        "repository": format!("{REPOSITORY_OWNER}/{REPOSITORY_NAME}"),
        "credentials": choices,
        "suggestedCredentialId": suggested,
        "previousReport": previous,
        "remainingToday": (REPORT_LIMIT - made).max(0),
    })))
}
