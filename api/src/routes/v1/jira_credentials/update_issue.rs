// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/jira-credentials/{id}/issues/{key}`: change an issue's fields.
//!
//! Jira answers a change with no content, so the issue is read back and returned as it is
//! afterwards.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{OpenCredential, TEXT_MAX_CHARACTERS, allowed_issue, open};
use crate::errors::ApiError;
use crate::jira::{Issue, IssueChanges, Jira};
use crate::state::AppState;

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
    let issue = change(&state.jira, &stored, &key, &changes).await?;

    Ok(Json(json!({ "issue": issue })))
}

/// Changes an issue's fields, refused unless the credential may touch the project it is in.
///
/// The issue is resolved before the change rather than after it, so a field of an issue
/// outside the allowlist is never overwritten and then reported `403`. It is read again
/// afterwards because the answer carries the issue as it is now, and that read checks the
/// project once more for free.
async fn change(
    jira: &Jira,
    stored: &OpenCredential,
    key: &str,
    changes: &IssueChanges,
) -> Result<Issue, ApiError> {
    allowed_issue(jira, stored, key).await?;

    jira.update_issue(&stored.site(), key, changes).await?;

    allowed_issue(jira, stored, key).await
}

#[cfg(test)]
mod tests {
    use reqwest::Method;

    use super::*;
    use crate::jira::tests::{fake_jira_site, sent};
    use crate::routes::v1::jira_credentials::tests::opened_for_test;

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

    #[tokio::test]
    async fn an_issue_that_moved_out_of_the_allowlist_is_never_changed() {
        let (jira, log) = fake_jira_site().await;
        let stored = opened_for_test(&["ELY"]);
        let changes = IssueChanges {
            summary: Some("Renamed".to_owned()),
            ..IssueChanges::default()
        };

        // ELY-99 answers with project SECRET: the key says one project and the issue is in
        // another, which is what an issue moved since the key was written looks like.
        let refused = change(&jira, &stored, "ELY-99", &changes)
            .await
            .unwrap_err();

        let ApiError::Forbidden(message) = refused else {
            panic!("an issue outside the allowlist is forbidden, not something else");
        };
        assert!(message.contains("SECRET"), "{message}");
        assert!(
            sent(&log)
                .iter()
                .all(|request| request.method == Method::GET),
            "an issue outside the allowlist is read and refused, never written to"
        );
    }
}
