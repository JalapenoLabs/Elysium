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

use super::{allowlist, open};
use crate::errors::ApiError;
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
    allowlist::issue_project(&stored.allowed.projects, &stored.credential.name, &key)?;

    let comment = state
        .jira
        .add_comment(&stored.site(), &key, &body.body)
        .await?;

    Ok((StatusCode::CREATED, Json(json!({ "comment": comment }))))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
