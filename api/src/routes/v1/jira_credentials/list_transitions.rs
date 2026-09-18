// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/jira-credentials/{id}/issues/{key}/transitions`: the moves an issue can
//! make right now, for whoever the stored token is.

use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{OpenCredential, allowed_issue, open};
use crate::errors::ApiError;
use crate::jira::{Jira, Transition};
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, String)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, key)) = path?;
    let stored = open(&state, id).await?;

    let transitions = transitions_of(&state.jira, &stored, &key).await?;

    Ok(Json(json!({ "transitions": transitions })))
}

/// The transitions an issue can make, refused unless the credential may touch its project.
///
/// A list of transitions names no project of its own, so the issue is resolved first and a
/// workflow outside the allowlist is never asked for.
async fn transitions_of(
    jira: &Jira,
    stored: &OpenCredential,
    key: &str,
) -> Result<Vec<Transition>, ApiError> {
    allowed_issue(jira, stored, key).await?;

    let transitions = jira.transitions(&stored.site(), key).await?;
    Ok(transitions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jira::tests::{fake_jira_site, sent};
    use crate::routes::v1::jira_credentials::tests::opened_for_test;

    #[tokio::test]
    async fn an_issue_that_moved_out_of_the_allowlist_has_no_transitions_read() {
        let (jira, log) = fake_jira_site().await;
        let stored = opened_for_test(&["ELY"]);

        // ELY-99 answers with project SECRET: the key says one project and the issue is in
        // another, which is what an issue moved since the key was written looks like.
        let refused = transitions_of(&jira, &stored, "ELY-99").await.unwrap_err();

        let ApiError::Forbidden(message) = refused else {
            panic!("an issue outside the allowlist is forbidden, not something else");
        };
        assert!(message.contains("SECRET"), "{message}");
        assert!(
            !sent(&log)
                .iter()
                .any(|request| request.path.ends_with("/transitions")),
            "a workflow outside the allowlist is never even asked for"
        );
    }
}
