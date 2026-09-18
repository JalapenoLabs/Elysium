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

use super::{OpenCredential, TEXT_MAX_CHARACTERS, allowed_issue, allowlist, open};
use crate::errors::ApiError;
use crate::jira::{Issue, Jira, NewIssue};
use crate::state::AppState;

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
    let issue = create(&state.jira, &stored, &new_issue).await?;

    Ok((StatusCode::CREATED, Json(json!({ "issue": issue }))))
}

/// Creates an issue, refused unless the credential may touch what the request names.
///
/// The project is named rather than an issue, so it is checked as a project. The parent is
/// an issue, so its key is not enough: an issue keeps its key when it moves project, and
/// Jira puts a child in its parent's project. It is resolved through [`allowed_issue`]
/// before the create, since a subtask landing in another project cannot be unmade.
async fn create(
    jira: &Jira,
    stored: &OpenCredential,
    new_issue: &NewIssue,
) -> Result<Issue, ApiError> {
    allowlist::ensure_allowed(
        &stored.allowed.projects,
        &stored.credential.name,
        &new_issue.project_key,
    )?;
    if let Some(parent_key) = &new_issue.parent_key {
        allowed_issue(jira, stored, parent_key).await?;
    }

    let key = jira.create_issue(&stored.site(), new_issue).await?;

    // Read back for the answer, which also proves Jira put the issue where it was asked to.
    allowed_issue(jira, stored, &key).await
}

#[cfg(test)]
mod tests {
    use reqwest::Method;

    use super::*;
    use crate::jira::tests::{fake_jira_site, sent};
    use crate::routes::v1::jira_credentials::tests::opened_for_test;

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

    #[tokio::test]
    async fn a_parent_that_moved_out_of_the_allowlist_creates_nothing() {
        let (jira, log) = fake_jira_site().await;
        let stored = opened_for_test(&["ELY"]);
        let new_issue = NewIssue {
            project_key: "ELY".to_owned(),
            issue_type: "Task".to_owned(),
            summary: "Under a parent that moved".to_owned(),
            // ELY-99 answers with project SECRET: a parent whose key still says ELY while
            // Jira has it in a project this credential was never given.
            parent_key: Some("ELY-99".to_owned()),
            ..NewIssue::default()
        };

        let refused = create(&jira, &stored, &new_issue).await.unwrap_err();

        let ApiError::Forbidden(message) = refused else {
            panic!("a parent outside the allowlist is forbidden, not something else");
        };
        assert!(message.contains("SECRET"), "{message}");
        assert!(
            sent(&log)
                .iter()
                .all(|request| request.method == Method::GET),
            "a child of an issue outside the allowlist is never created"
        );
    }
}
