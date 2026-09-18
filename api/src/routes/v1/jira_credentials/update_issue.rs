// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/jira-credentials/{id}/issues/{key}`: change an issue's fields.
//!
//! Jira answers a change with no content, so the issue is read back and returned as it is
//! afterwards. That read also proves the issue is still in an allowed project.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{allowlist, open};
use crate::errors::ApiError;
use crate::jira::IssueChanges;
use crate::state::AppState;

/// The longest description a client may send, as when creating an issue.
const TEXT_MAX_CHARACTERS: u64 = 32_768;

/// Absent fields stay as they are.
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 255))]
    summary: Option<String>,
    /// Plain text, wrapped into Atlassian Document Format before it is sent.
    #[validate(length(max = TEXT_MAX_CHARACTERS))]
    description: Option<String>,
    labels: Option<Vec<String>>,
    priority: Option<String>,
    /// `null` unassigns the issue; absent leaves the assignee alone.
    #[expect(
        clippy::option_option,
        reason = "absent, unassign, and assign are three distinct requests"
    )]
    #[serde(default, with = "::serde_with::rust::double_option")]
    assignee_account_id: Option<Option<String>>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, String)>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, key)) = path?;
    let Json(body) = body?;
    body.validate()?;

    let changes = IssueChanges {
        summary: body.summary,
        description: body.description,
        labels: body.labels,
        priority: body.priority,
        assignee_account_id: body.assignee_account_id,
    };
    if changes.is_empty() {
        return Err(ApiError::BadRequest(
            "request body contains no fields to update".to_owned(),
        ));
    }

    let stored = open(&state, id).await?;
    let projects = &stored.allowed.projects;
    let name = &stored.credential.name;
    allowlist::issue_project(projects, name, &key)?;

    state
        .jira
        .update_issue(&stored.site(), &key, &changes)
        .await?;
    let issue = state.jira.issue(&stored.site(), &key).await?;
    allowlist::ensure_allowed(projects, name, &issue.project_key)?;

    Ok(Json(json!({ "issue": issue })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_assignee_and_a_null_one_are_different_requests() {
        let absent: RequestBody =
            serde_json::from_value(json!({ "summary": "New" })).expect("parses");
        assert_eq!(absent.assignee_account_id, None);

        let unassign: RequestBody =
            serde_json::from_value(json!({ "assigneeAccountId": null })).expect("parses");
        assert_eq!(unassign.assignee_account_id, Some(None));

        let assign: RequestBody =
            serde_json::from_value(json!({ "assigneeAccountId": "5b10" })).expect("parses");
        assert_eq!(assign.assignee_account_id, Some(Some("5b10".to_owned())));
    }

    #[test]
    fn an_empty_body_changes_nothing() {
        let empty: RequestBody = serde_json::from_value(json!({})).expect("parses");
        empty.validate().expect("nothing to validate");

        let changes = IssueChanges {
            summary: empty.summary,
            description: empty.description,
            labels: empty.labels,
            priority: empty.priority,
            assignee_account_id: empty.assignee_account_id,
        };
        assert!(
            changes.is_empty(),
            "the handler refuses this before calling Jira"
        );
    }
}
