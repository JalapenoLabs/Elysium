// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/issue-reports`: file a report on Elysium's GitHub repository with one of the
//! reporter's own tokens, as a comment on the problem's open issue or as a new one.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::{HeaderMap, StatusCode};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{Report, environment};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::issue_reports::{self, Readiness};
use crate::models::github_credential;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestBody {
    #[serde(flatten)]
    report: Report,
    /// The reporter's own token to file it with.
    credential_id: Uuid,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    headers: HeaderMap,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    let note = body.report.note()?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let credential = github_credential::find(&mut connection, body.credential_id).await?;
    drop(connection);
    // A report is filed as the person who makes it, so only a token they added may file it.
    if credential.created_by != current.id() {
        return Err(ApiError::NotFound);
    }
    let readiness = issue_reports::readiness(&credential, Utc::now());
    if !readiness.may_try() {
        let message = match readiness {
            Readiness::Expired => "this token has expired; add a new one, or choose another",
            _ => {
                "this token cannot open issues on public repositories; give it the public_repo \
                 scope, or choose another"
            }
        };
        return Err(ApiError::CodedBadRequest {
            code: "github_token_cannot_report",
            message: message.to_owned(),
        });
    }
    let token = credential
        .token(&state.cipher)
        .context("the stored token cannot be decrypted")?;

    let draft = issue_reports::prepare(
        &state.database,
        &state.connectors.check_runs,
        current.id(),
        &body.report.subject,
        note,
        &environment(&state, &headers),
    )
    .await?;
    let filed = state
        .issue_reports
        .file(&state.database, &state.github, current.id(), &token, &draft)
        .await?;

    Ok((StatusCode::CREATED, Json(json!({ "report": filed }))))
}
