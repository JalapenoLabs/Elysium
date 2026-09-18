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

use super::{OpenCredential, TEXT_MAX_CHARACTERS, allowed_issue, open};
use crate::errors::ApiError;
use crate::jira::{Issue, Jira};
use crate::state::AppState;

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
    let issue = transition(&state.jira, &stored, &key, &body).await?;

    Ok(Json(json!({ "issue": issue })))
}

/// Moves an issue, refused unless the credential may touch the project it is in.
///
/// The issue is resolved before the move rather than after it. A transition is the least
/// reversible thing Elysium asks of Jira: it changes the issue's state and fires whatever
/// the project automates on that workflow, none of which a later `403` takes back.
///
/// It is read again afterwards, since the answer carries the issue's new status, and that
/// read checks the project once more for free.
async fn transition(
    jira: &Jira,
    stored: &OpenCredential,
    key: &str,
    body: &RequestBody,
) -> Result<Issue, ApiError> {
    allowed_issue(jira, stored, key).await?;

    jira.apply_transition(
        &stored.site(),
        key,
        &body.transition_id,
        body.comment.as_deref(),
    )
    .await?;

    allowed_issue(jira, stored, key).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jira::tests::{fake_jira_site, sent};
    use crate::routes::v1::jira_credentials::tests::opened_for_test;

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

    #[tokio::test]
    async fn an_issue_that_moved_out_of_the_allowlist_is_never_transitioned() {
        let (jira, log) = fake_jira_site().await;
        let stored = opened_for_test(&["ELY"]);
        let body: RequestBody =
            serde_json::from_value(json!({ "transitionId": "31" })).expect("parses");

        // ELY-99 answers with project SECRET: the key says one project and the issue is in
        // another, which is what an issue moved since the key was written looks like.
        let refused = transition(&jira, &stored, "ELY-99", &body)
            .await
            .unwrap_err();

        let ApiError::Forbidden(message) = refused else {
            panic!("an issue outside the allowlist is forbidden, not something else");
        };
        assert!(message.contains("SECRET"), "{message}");
        assert!(
            !sent(&log)
                .iter()
                .any(|request| request.path.ends_with("/transitions")),
            "an issue outside the allowlist is never moved, so no automation of its project \
             fires either"
        );
    }
}
