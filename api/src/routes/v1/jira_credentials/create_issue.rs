// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/jira-credentials/{id}/issues`: create an issue in an allowed project.
//!
//! Jira answers a creation with the new key and nothing else, so the issue is read back and
//! returned whole. That read also checks the project Jira put it in, which is what an issue
//! created under a parent in another project would show.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{allowlist, open};
use crate::errors::ApiError;
use crate::jira::NewIssue;
use crate::state::AppState;

/// The longest description a client may send. Jira's own ceiling is far higher; this keeps
/// one request from carrying a document nobody meant to paste.
const TEXT_MAX_CHARACTERS: u64 = 32_768;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 2, max = 255))]
    project_key: String,
    #[validate(length(min = 1, max = 255))]
    issue_type: String,
    #[validate(length(min = 1, max = 255))]
    summary: String,
    /// Plain text, wrapped into Atlassian Document Format before it is sent.
    #[validate(length(max = TEXT_MAX_CHARACTERS))]
    description: Option<String>,
    labels: Option<Vec<String>>,
    priority: Option<String>,
    assignee_account_id: Option<String>,
    /// The epic or parent issue this one belongs under, which must also be allowed.
    parent_key: Option<String>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    body.validate()?;

    let stored = open(&state, id).await?;
    let projects = &stored.allowed.projects;
    let name = &stored.credential.name;
    allowlist::ensure_allowed(projects, name, &body.project_key)?;
    if let Some(parent_key) = &body.parent_key {
        allowlist::issue_project(projects, name, parent_key)?;
    }

    let new_issue = NewIssue {
        project_key: body.project_key,
        issue_type: body.issue_type,
        summary: body.summary,
        description: body.description,
        labels: body.labels,
        priority: body.priority,
        assignee_account_id: body.assignee_account_id,
        parent_key: body.parent_key,
    };
    let key = state.jira.create_issue(&stored.site(), &new_issue).await?;

    let issue = state.jira.issue(&stored.site(), &key).await?;
    allowlist::ensure_allowed(projects, name, &issue.project_key)?;

    Ok((StatusCode::CREATED, Json(json!({ "issue": issue }))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_issue_parses_with_its_optional_fields() {
        let body: RequestBody = serde_json::from_value(json!({
            "projectKey": "ELY",
            "issueType": "Task",
            "summary": "Bound every search to the allowlist",
            "description": "One paragraph.\n\nAnd another.",
            "labels": ["backend"],
        }))
        .expect("parses");

        body.validate().expect("valid");
        assert_eq!(
            body.labels.as_deref(),
            Some(["backend".to_owned()].as_slice())
        );
        assert!(body.priority.is_none() && body.parent_key.is_none());
    }

    #[test]
    fn a_summary_is_required_and_bounded() {
        serde_json::from_value::<RequestBody>(json!({ "projectKey": "ELY", "issueType": "Task" }))
            .expect_err("an issue needs a summary");

        let long: RequestBody = serde_json::from_value(json!({
            "projectKey": "ELY",
            "issueType": "Task",
            "summary": "s".repeat(256),
        }))
        .expect("parses");
        assert!(
            long.validate()
                .expect_err("an oversized summary")
                .field_errors()
                .contains_key("summary")
        );
    }
}
