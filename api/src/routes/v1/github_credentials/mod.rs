// Copyright © 2026 Jalapeno Labs

//! `/api/v1/github-credentials`: the GitHub tokens Elysium holds. Tokens are write-only
//! over HTTP, and every write checks the token with GitHub before it is stored.

mod check_repository_access;
mod create_github_credential;
mod delete_github_credential;
mod get_github_credential;
mod list_github_credentials;
mod list_repositories;
mod list_repository_issues;
mod list_repository_labels;
mod list_repository_milestones;
mod test_github_credential;
mod update_github_credential;

use axum::Router;
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;
use validator::ValidationError;

use crate::errors::ApiError;
use crate::github::Repository;
use crate::models::github_credential::{GithubCredential, GithubTokenKind};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(list_github_credentials::handle).post(create_github_credential::handle),
        )
        .route(
            "/{id}",
            get(get_github_credential::handle)
                .patch(update_github_credential::handle)
                .delete(delete_github_credential::handle),
        )
        .route("/{id}/test", post(test_github_credential::handle))
        .route("/{id}/repositories", get(list_repositories::handle))
        .route(
            "/{id}/repository-access",
            post(check_repository_access::handle),
        )
        .route(
            "/{id}/repositories/{owner}/{name}/issues",
            get(list_repository_issues::handle),
        )
        .route(
            "/{id}/repositories/{owner}/{name}/milestones",
            get(list_repository_milestones::handle),
        )
        .route(
            "/{id}/repositories/{owner}/{name}/labels",
            get(list_repository_labels::handle),
        )
}

/// How many pages of milestones or labels a picker reads, so 500 of each. A repository
/// with more is rare; the listing says when it was cut short.
const CONTAINER_PAGE_LIMIT: usize = 5;

/// The repository an owner and name in a path name, checked against GitHub's alphabets
/// before either reaches a URL.
///
/// # Errors
/// Returns [`ApiError::BadRequest`] for an owner or name GitHub would not accept.
fn repository_of(owner: &str, name: &str) -> Result<Repository, ApiError> {
    Repository::from_url(&format!("https://github.com/{owner}/{name}")).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "{owner}/{name} is not a repository name GitHub accepts"
        ))
    })
}

/// Upper bound on a stored token. A fine-grained token is about 93 characters today;
/// this leaves room for longer ones without letting a row carry an arbitrary blob.
const TOKEN_MAX_BYTES: usize = 255;

/// The 40 characters a classic token had before GitHub gave them the `ghp_` prefix in
/// 2021. Those tokens still work, so they are still accepted.
const LEGACY_TOKEN_LENGTH: usize = 40;

/// A credential as clients see it. The token is never included, sealed or not.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubCredentialResponse {
    id: Uuid,
    name: String,
    kind: GithubTokenKind,
    /// The account the token acts as.
    login: String,
    /// A classic token's scopes; empty for a fine-grained token.
    scopes: Vec<String>,
    /// When GitHub stops accepting the token, or `null` for one that does not expire.
    token_expires_at: Option<DateTime<Utc>>,
    /// When GitHub last confirmed the token.
    checked_at: DateTime<Utc>,
    /// The workspace default, which sessions start with unless their project or the
    /// session chooses otherwise.
    is_default: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl GithubCredentialResponse {
    pub fn new(credential: GithubCredential) -> Self {
        Self {
            id: credential.id,
            name: credential.name,
            kind: credential.kind,
            login: credential.login,
            scopes: credential
                .scopes
                .split(',')
                .filter(|scope| !scope.is_empty())
                .map(str::to_owned)
                .collect(),
            token_expires_at: credential.token_expires_at,
            checked_at: credential.checked_at,
            is_default: credential.is_default,
            created_at: credential.created_at,
            updated_at: credential.updated_at,
        }
    }
}

/// Whether a token can push to a repository its account may or may not push to.
///
/// A classic token pushes where its account may push and its scopes allow: `repo` for any
/// repository, or `public_repo` for a public one. A fine-grained token's own permissions are
/// not reported, so its push access is `None`, never guessed from the account.
fn token_can_push(
    kind: GithubTokenKind,
    role_can_push: bool,
    is_private: bool,
    scopes: &[String],
) -> Option<bool> {
    match kind {
        GithubTokenKind::Classic => {
            let has_scope = |wanted: &str| scopes.iter().any(|scope| scope == wanted);
            let scope_allows = has_scope("repo") || (!is_private && has_scope("public_repo"));
            Some(role_can_push && scope_allows)
        }
        GithubTokenKind::FineGrained => None,
    }
}

/// Rejects names that are only whitespace; `length` alone would accept `"   "`.
fn validate_not_blank(value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::new("blank").with_message("must not be blank".into()));
    }
    Ok(())
}

/// Whether `token` is written the way GitHub writes a token of this kind.
///
/// Lengths are GitHub's to change, so only the prefix and the alphabet are checked; the
/// token itself is proven by asking GitHub. The legacy form has no prefix, which is why a
/// bare 40 characters still counts as classic.
fn matches_kind(kind: GithubTokenKind, token: &SecretString) -> bool {
    let token = token.expose_secret();
    match kind {
        GithubTokenKind::Classic => {
            let is_prefixed = token.strip_prefix("ghp_").is_some_and(|rest| {
                !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric())
            });
            let is_legacy = token.len() == LEGACY_TOKEN_LENGTH
                && token.chars().all(|character| character.is_ascii_hexdigit());
            is_prefixed || is_legacy
        }
        GithubTokenKind::FineGrained => token.strip_prefix("github_pat_").is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
        }),
    }
}

/// What to tell a client whose token is not written the way its kind is.
const fn token_shape_message(kind: GithubTokenKind) -> &'static str {
    match kind {
        GithubTokenKind::Classic => {
            "a classic token starts with ghp_, or is the 40 characters GitHub issued before 2021"
        }
        GithubTokenKind::FineGrained => "a fine-grained token starts with github_pat_",
    }
}

/// A token from a request body, checked as it is parsed.
///
/// Validation happens during deserialization rather than through `validator`, because a
/// validator error report would need to serialize the value itself. Surrounding
/// whitespace is dropped, since a token copied out of a console often brings some and
/// never contains any.
#[derive(Debug)]
pub struct GithubToken(pub SecretString);

impl<'de> Deserialize<'de> for GithubToken {
    fn deserialize<Source: Deserializer<'de>>(deserializer: Source) -> Result<Self, Source::Error> {
        let token = String::deserialize(deserializer)?;
        let token = token.trim();

        if token.is_empty() {
            return Err(serde::de::Error::custom("token must not be blank"));
        }
        if token.len() > TOKEN_MAX_BYTES {
            return Err(serde::de::Error::custom("token exceeds 255 characters"));
        }

        Ok(Self(SecretString::from(token)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_tokens_are_prefixed_or_the_legacy_forty_characters() {
        let token = |value: &str| SecretString::from(value);

        assert!(matches_kind(
            GithubTokenKind::Classic,
            &token("ghp_16C7e42F292c6912E7710c838347Ae178B4a")
        ));
        assert!(matches_kind(
            GithubTokenKind::Classic,
            &token(&"a1b2c3d4".repeat(5))
        ));
        assert!(!matches_kind(GithubTokenKind::Classic, &token("ghp_")));
        assert!(!matches_kind(
            GithubTokenKind::Classic,
            &token("github_pat_11ABCDEFG0abcdefg")
        ));
        assert!(!matches_kind(
            GithubTokenKind::Classic,
            &token("ghp_has a space")
        ));
    }

    #[test]
    fn fine_grained_tokens_carry_githubs_own_prefix() {
        let token = |value: &str| SecretString::from(value);

        assert!(matches_kind(
            GithubTokenKind::FineGrained,
            &token("github_pat_11ABCDEFG0abcdefg_HIJKLMN")
        ));
        assert!(!matches_kind(
            GithubTokenKind::FineGrained,
            &token("ghp_16C7e42F292c6912E7710c838347Ae178B4a")
        ));
        assert!(!matches_kind(
            GithubTokenKind::FineGrained,
            &token("github_pat_")
        ));
    }

    #[test]
    fn tokens_are_trimmed_and_bounded_while_parsing() {
        let parsed: GithubToken =
            serde_json::from_value(serde_json::json!("  ghp_padded  ")).expect("parses");
        assert_eq!(parsed.0.expose_secret(), "ghp_padded");

        serde_json::from_value::<GithubToken>(serde_json::json!("   ")).expect_err("blank token");
        serde_json::from_value::<GithubToken>(serde_json::json!("g".repeat(256)))
            .expect_err("an oversized token");

        assert!(!format!("{parsed:?}").contains("padded"));
    }
}
