// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/github-credentials/{id}/repositories`: the repositories a token can see, for
//! picking a session's repositories from a list instead of pasting their URLs.
//!
//! Nothing is cached. GitHub answers a page of a hundred in about a second, and a listing
//! is asked for once when the new session form opens on a token.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::token_can_push;
use crate::errors::ApiError;
use crate::github::ListedRepository;
use crate::models::github_credential::{self, GithubTokenKind};
use crate::state::AppState;

/// One repository as clients see it.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RepositoryResponse {
    full_name: String,
    owner: String,
    name: String,
    private: bool,
    archived: bool,
    default_branch: String,
    clone_url: String,
    pushed_at: Option<DateTime<Utc>>,
    /// Whether the token can push: `null` for a fine-grained token, whose permissions GitHub
    /// does not report.
    can_push: Option<bool>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let credential = github_credential::find(&mut connection, id).await?;
    drop(connection);

    let token = credential
        .token(&state.cipher)
        .context("the stored token cannot be decrypted")?;
    let listing = state.github.list_repositories(&token).await?;

    let repositories: Vec<RepositoryResponse> = listing
        .repositories
        .into_iter()
        .map(|repository| describe(repository, credential.kind, &listing.scopes))
        .collect();
    Ok(Json(json!({
        "repositories": repositories,
        "truncated": listing.truncated,
    })))
}

fn describe(
    repository: ListedRepository,
    kind: GithubTokenKind,
    scopes: &[String],
) -> RepositoryResponse {
    RepositoryResponse {
        can_push: token_can_push(
            kind,
            repository.role_can_push,
            repository.is_private,
            scopes,
        ),
        full_name: repository.full_name,
        owner: repository.owner,
        name: repository.name,
        private: repository.is_private,
        archived: repository.is_archived,
        default_branch: repository.default_branch,
        clone_url: repository.clone_url,
        pushed_at: repository.pushed_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(is_private: bool, role_can_push: bool) -> ListedRepository {
        ListedRepository {
            full_name: "JalapenoLabs/Elysium".to_owned(),
            owner: "JalapenoLabs".to_owned(),
            name: "Elysium".to_owned(),
            is_private,
            is_archived: false,
            default_branch: "main".to_owned(),
            clone_url: "https://github.com/JalapenoLabs/Elysium.git".to_owned(),
            pushed_at: None,
            role_can_push,
        }
    }

    #[test]
    fn listed_repositories_report_push_access_the_way_a_single_check_does() {
        let repo_scope = ["repo".to_owned()];

        let classic = describe(listed(true, true), GithubTokenKind::Classic, &repo_scope);
        assert_eq!(classic.can_push, Some(true));
        assert_eq!(
            classic.clone_url,
            "https://github.com/JalapenoLabs/Elysium.git"
        );

        let read_only = describe(listed(true, false), GithubTokenKind::Classic, &repo_scope);
        assert_eq!(read_only.can_push, Some(false));

        let fine_grained = describe(listed(true, true), GithubTokenKind::FineGrained, &[]);
        assert_eq!(fine_grained.can_push, None);
    }
}
