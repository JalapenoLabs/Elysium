// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/jira-credentials/{id}/issues/{key}/comments`: comment on an issue.
//!
//! The comment is written as plain text and sent as Atlassian Document Format. What comes
//! back carries both, as everywhere Elysium returns rich text.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{OpenCredential, allowlist, open};
use crate::errors::ApiError;
use crate::jira::{Comment, Jira};
use crate::state::AppState;

/// The longest comment a client may send.
const TEXT_MAX_CHARACTERS: u64 = 32_768;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = TEXT_MAX_CHARACTERS))]
    body: String,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, String)>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Path((id, key)) = path?;
    let Json(body) = body?;
    body.validate()?;

    let stored = open(&state, id).await?;
    let comment = comment_on(&state.jira, &stored, &key, &body.body).await?;

    Ok((StatusCode::CREATED, Json(json!({ "comment": comment }))))
}

/// Comments on an issue, refused unless the credential may touch the project it is in.
///
/// A comment names no project of its own, so the issue is read first and the project Jira
/// reports for it is checked, which is what every other issue route checks on its answer.
/// The key alone is not enough: an issue keeps its old key when it moves project, so
/// `ELY-99` can resolve to an issue that now lives in one this credential was never given.
/// Here the check comes before the write, since a comment cannot be taken back.
async fn comment_on(
    jira: &Jira,
    stored: &OpenCredential,
    key: &str,
    text: &str,
) -> Result<Comment, ApiError> {
    let projects = &stored.allowed.projects;
    let name = &stored.credential.name;
    allowlist::issue_project(projects, name, key)?;

    let issue = jira.issue(&stored.site(), key).await?;
    allowlist::ensure_allowed(projects, name, &issue.project_key)?;

    let comment = jira.add_comment(&stored.site(), key, text).await?;
    Ok(comment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jira::tests::{fake_jira_site, sent};
    use crate::routes::v1::jira_credentials::opened_for_test;

    #[test]
    fn a_comment_needs_a_body() {
        let body: RequestBody =
            serde_json::from_value(json!({ "body": "Looks right" })).expect("parses");
        body.validate().expect("valid");

        let empty: RequestBody = serde_json::from_value(json!({ "body": "" })).expect("parses");
        assert!(
            empty
                .validate()
                .expect_err("an empty comment")
                .field_errors()
                .contains_key("body")
        );
    }

    #[tokio::test]
    async fn an_issue_that_moved_out_of_the_allowlist_is_never_commented_on() {
        let (jira, log) = fake_jira_site().await;
        let stored = opened_for_test(&["ELY"]);

        // ELY-99 answers with project SECRET: the key says one project and the issue is in
        // another, which is what an issue moved since the key was written looks like.
        let refused = comment_on(&jira, &stored, "ELY-99", "Looks right")
            .await
            .unwrap_err();

        let ApiError::Forbidden(message) = refused else {
            panic!("an issue outside the allowlist is forbidden, not something else");
        };
        assert!(message.contains("SECRET"), "{message}");
        assert!(
            !sent(&log)
                .iter()
                .any(|request| request.path.ends_with("/comment")),
            "nothing is written to an issue outside the allowlist"
        );
    }
}
