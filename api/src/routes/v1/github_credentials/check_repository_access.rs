// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/github-credentials/{id}/repository-access`: whether a token can reach the
//! repository a session is about to clone, asked before the session is created.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use crate::errors::ApiError;
use crate::github::{Repository, RepositoryAccess};
use crate::models::github_credential::{self, GithubTokenKind};
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 2048))]
    repository_url: String,
}

/// What the token can do with the repository.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Access {
    /// `owner/name` as the URL names it.
    repository: String,
    /// Whether the token can see the repository at all.
    can_read: bool,
    /// Whether the token can push: `null` when that cannot be known, which is always the
    /// case for a fine-grained token, since GitHub does not report its permissions.
    can_push: Option<bool>,
    /// `null` when the token cannot see the repository.
    is_private: Option<bool>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    body.validate()?;

    let repository = Repository::from_url(&body.repository_url).ok_or_else(|| {
        ApiError::BadRequest("repositoryUrl is not a repository on github.com".to_owned())
    })?;

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
    let found = state.github.repository_access(&token, &repository).await?;

    Ok(Json(json!({
        "result": describe(&repository, credential.kind, found.as_ref()),
    })))
}

/// Turns GitHub's answer into what the token can do.
///
/// A classic token pushes where its account may push and its scopes allow: `repo` for any
/// repository, or `public_repo` for a public one. A fine-grained token's permissions are
/// not reported, so its push access is left unknown rather than guessed from the account.
fn describe(
    repository: &Repository,
    kind: GithubTokenKind,
    found: Option<&RepositoryAccess>,
) -> Access {
    let name = format!("{}/{}", repository.owner, repository.name);
    let Some(found) = found else {
        return Access {
            repository: name,
            can_read: false,
            can_push: Some(false),
            is_private: None,
        };
    };

    let can_push = match kind {
        GithubTokenKind::Classic => {
            let has_scope = |wanted: &str| found.scopes.iter().any(|scope| scope == wanted);
            let scope_allows = has_scope("repo") || (!found.is_private && has_scope("public_repo"));
            Some(found.role_can_push && scope_allows)
        }
        GithubTokenKind::FineGrained => None,
    };
    Access {
        repository: found.full_name.clone(),
        can_read: true,
        can_push,
        is_private: Some(found.is_private),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elysium() -> Repository {
        Repository {
            owner: "JalapenoLabs".to_owned(),
            name: "Elysium".to_owned(),
        }
    }

    fn found(is_private: bool, role_can_push: bool, scopes: &[&str]) -> RepositoryAccess {
        RepositoryAccess {
            full_name: "JalapenoLabs/Elysium".to_owned(),
            is_private,
            role_can_push,
            scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
        }
    }

    #[test]
    fn a_hidden_repository_can_be_neither_read_nor_pushed() {
        let access = describe(&elysium(), GithubTokenKind::Classic, None);
        assert!(!access.can_read);
        assert_eq!(access.can_push, Some(false));
        assert_eq!(access.is_private, None);
    }

    #[test]
    fn classic_tokens_push_where_both_the_account_and_the_scopes_allow() {
        let classic = GithubTokenKind::Classic;
        let private =
            |role, scopes| describe(&elysium(), classic, Some(&found(true, role, scopes)));

        assert_eq!(private(true, &["repo"]).can_push, Some(true));
        assert_eq!(private(false, &["repo"]).can_push, Some(false));
        assert_eq!(private(true, &["public_repo"]).can_push, Some(false));
        assert_eq!(
            describe(
                &elysium(),
                classic,
                Some(&found(false, true, &["public_repo"]))
            )
            .can_push,
            Some(true)
        );
    }

    #[test]
    fn fine_grained_push_access_is_never_guessed() {
        let access = describe(
            &elysium(),
            GithubTokenKind::FineGrained,
            Some(&found(true, true, &[])),
        );
        assert!(access.can_read);
        assert_eq!(access.can_push, None);
    }
}
