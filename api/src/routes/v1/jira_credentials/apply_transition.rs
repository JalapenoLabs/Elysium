// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/jira-credentials/{id}/issues/{key}/transitions`: move an issue along one
//! transition, optionally leaving a comment with it.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{allowlist, open};
use crate::errors::ApiError;
use crate::state::AppState;

/// The longest comment a client may send with a transition.
const TEXT_MAX_CHARACTERS: u64 = 32_768;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// One of the ids `GET .../transitions` answered.
    #[validate(length(min = 1, max = 64))]
    transition_id: String,
    /// Plain text, added to the issue as part of the move.
    #[validate(length(max = TEXT_MAX_CHARACTERS))]
    comment: Option<String>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, String)>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, key)) = path?;
    let Json(body) = body?;
    body.validate()?;

    let stored = open(&state, id).await?;
    let projects = &stored.allowed.projects;
    let name = &stored.credential.name;
    allowlist::issue_project(projects, name, &key)?;

    state
        .jira
        .apply_transition(
            &stored.site(),
            &key,
            &body.transition_id,
            body.comment.as_deref(),
        )
        .await?;

    // Read back, both to answer with the issue's new status and to prove it is still in a
    // project this credential may touch.
    let issue = state.jira.issue(&stored.site(), &key).await?;
    allowlist::ensure_allowed(projects, name, &issue.project_key)?;

    Ok(Json(json!({ "issue": issue })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transition_needs_an_id_and_may_carry_a_comment() {
        let body: RequestBody =
            serde_json::from_value(json!({ "transitionId": "31" })).expect("parses");
        body.validate().expect("valid");
        assert!(body.comment.is_none());

        let with_comment: RequestBody =
            serde_json::from_value(json!({ "transitionId": "31", "comment": "Shipping it" }))
                .expect("parses");
        assert_eq!(with_comment.comment.as_deref(), Some("Shipping it"));

        serde_json::from_value::<RequestBody>(json!({ "comment": "no id" }))
            .expect_err("a transition needs its id");
    }
}
