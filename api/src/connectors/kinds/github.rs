// Copyright © 2026 Jalapeno Labs

//! GitHub: issues and pull requests, read through a GitHub account by the link watcher. See
//! `docs/github.md`.
//!
//! It reads whatever its token reaches, so it has no content of its own. A GitHub account
//! carries exactly one GitHub connector, made with it, since links can be made through any
//! token and the watcher reads through each; a new GitHub connector is always a new account.

use secrecy::SecretString;
use serde_json::Value;

use super::{AccountChoice, AccountDefinition, AccountField, AccountFieldType, Definition, Source};
use crate::auth::CurrentUser;
use crate::connectors::checks::{CheckSpec, Checks, Outcome};
use crate::connectors::{AccountKind, ConnectorKind, Scope};
use crate::errors::ApiError;
use crate::github::issues::{Issue, IssueQuery};
use crate::github::{Github, ListedRepository, Repository};
use crate::models::connector::{self, AccountRef, Connector};
use crate::models::github_credential;
use crate::realtime::ServerEvent;
use crate::routes::v1::github_credentials::GithubToken;
use crate::routes::v1::github_credentials::create_github_credential::{self, RequestBody};
use crate::state::AppState;

pub const DEFINITION: Definition = Definition {
    kind: ConnectorKind::Github,
    scope: Scope::Workspace,
    account: AccountKind::Github,
    start_url: None,
    content: &[],
    checks: &[
        CheckSpec {
            id: "sign_in",
            required: true,
            depends_on: &[],
        },
        CheckSpec {
            id: "read_scopes",
            required: false,
            depends_on: &["sign_in"],
        },
        CheckSpec {
            id: "list_repositories",
            required: true,
            depends_on: &["sign_in"],
        },
        CheckSpec {
            id: "sample_issues",
            required: false,
            depends_on: &["list_repositories"],
        },
        CheckSpec {
            id: "sample_pull_requests",
            required: false,
            depends_on: &["list_repositories"],
        },
    ],
};

pub const ACCOUNT: AccountDefinition = AccountDefinition {
    kind: AccountKind::Github,
    fields: &[
        AccountField {
            id: "name",
            field: AccountFieldType::Text,
            required: true,
        },
        AccountField {
            id: "kind",
            field: AccountFieldType::Choice {
                options: &["classic", "fine-grained"],
            },
            required: true,
        },
        AccountField {
            id: "token",
            field: AccountFieldType::Secret,
            required: true,
        },
    ],
};

/// A new GitHub account's token, for checks before anything is stored.
#[derive(Debug, serde::Deserialize)]
struct NewToken {
    token: GithubToken,
}

pub async fn prepare_checks(
    state: &AppState,
    source: Source<'_>,
) -> Result<GithubChecks, ApiError> {
    let token = match source {
        Source::Stored(stored) => {
            let id = stored.github_credential_id.ok_or_else(|| {
                ApiError::BadRequest("a GitHub connector reads through a GitHub account".to_owned())
            })?;
            stored_token(state, id).await?
        }
        Source::Draft { account, .. } => match account {
            AccountChoice::Existing { id } => stored_token(state, id).await?,
            AccountChoice::New { new } => {
                let fields: NewToken = super::parse_new_account(AccountKind::Github, new)?;
                fields.token.0
            }
        },
    };
    Ok(GithubChecks {
        github: state.github.clone(),
        token,
        scopes: Vec::new(),
        repositories: Vec::new(),
        sample: None,
    })
}

async fn stored_token(state: &AppState, id: uuid::Uuid) -> Result<SecretString, ApiError> {
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let credential = github_credential::find(&mut connection, id).await?;
    let token = credential
        .token(&state.cipher)
        .map_err(|_open_error| anyhow::anyhow!("the stored token cannot be decrypted"))?;
    Ok(token)
}

/// GitHub's checks against one token.
#[derive(Debug)]
pub struct GithubChecks {
    github: Github,
    token: SecretString,
    scopes: Vec<String>,
    repositories: Vec<ListedRepository>,
    /// The most recently pushed repository's latest issues and pull requests, read once for
    /// both samples.
    sample: Option<Result<Vec<Issue>, String>>,
}

impl Checks for GithubChecks {
    async fn check(&mut self, id: &'static str) -> Outcome {
        match id {
            "sign_in" => match self.github.verify(&self.token).await {
                Ok(account) => {
                    self.scopes = account.scopes;
                    Outcome::passed()
                }
                Err(error) => Outcome::failed(error.to_string()),
            },
            "read_scopes" => {
                if self.scopes.is_empty() {
                    // A fine-grained token's permissions are granted per repository, and GitHub
                    // does not report them.
                    return Outcome::skipped("fine_grained_scopes");
                }
                Outcome {
                    message: Some(self.scopes.join(", ")),
                    ..Outcome::counted(self.scopes.len())
                }
            }
            "list_repositories" => match self.github.list_repositories(&self.token).await {
                Ok(listing) => {
                    self.repositories = listing.repositories;
                    Outcome::counted(self.repositories.len())
                }
                Err(error) => Outcome::failed(error.to_string()),
            },
            "sample_issues" => self.sample(|issue| issue.pull_request.is_none()).await,
            "sample_pull_requests" => self.sample(|issue| issue.pull_request.is_some()).await,
            unknown => Outcome::failed(format!("GitHub has no check {unknown}")),
        }
    }
}

impl GithubChecks {
    /// How many of the sampled repository's latest issues and pull requests `counts`.
    async fn sample(&mut self, counts: impl Fn(&Issue) -> bool) -> Outcome {
        let Some(repository) = self.repositories.first() else {
            return Outcome::skipped("no_repositories");
        };
        if self.sample.is_none() {
            let target = Repository {
                owner: repository.owner.clone(),
                name: repository.name.clone(),
            };
            let listed = self
                .github
                .list_issues(&self.token, &target, &IssueQuery::default(), 1)
                .await
                .map(|page| page.items)
                .map_err(|error| error.to_string());
            self.sample = Some(listed);
        }
        match &self.sample {
            Some(Ok(issues)) => Outcome {
                message: Some(repository.full_name.clone()),
                ..Outcome::counted(issues.iter().filter(|issue| counts(issue)).count())
            },
            Some(Err(message)) => Outcome::failed(message.clone()),
            None => Outcome::failed_because("sample_unread"),
        }
    }
}

/// A GitHub connector comes with a new GitHub account, made the way Settings, GitHub makes one.
///
/// # Errors
/// [`ApiError::CodedConflict`] for an existing account, which already carries its connector;
/// otherwise as [`create_github_credential::create_credential`].
pub async fn create(
    state: &AppState,
    current: &CurrentUser,
    account: AccountChoice,
    content: Value,
) -> Result<Connector, ApiError> {
    let AccountChoice::New { new } = account else {
        return Err(ApiError::CodedConflict {
            code: "account_has_connector",
            message: "a GitHub account already carries its GitHub connector",
        });
    };
    if content.as_object().is_some_and(|fields| !fields.is_empty()) {
        return Err(ApiError::BadRequest(
            "github reads everything its token reaches and takes no content".to_owned(),
        ));
    }
    let body: RequestBody = serde_json::from_value(new).map_err(|error| {
        ApiError::BadRequest(format!("a new GitHub account is not valid: {error}"))
    })?;
    let credential = create_github_credential::create_credential(state, current.id(), body).await?;

    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let found = connector::find_by_account(
        &mut connection,
        ConnectorKind::Github.as_str(),
        AccountRef::Github(credential.id),
    )
    .await?;
    found.ok_or(ApiError::NotFound)
}

/// Removes the connector with its GitHub account, as Settings, GitHub removes one.
///
/// # Errors
/// [`ApiError::NotFound`] when the account was already removed.
pub async fn remove(state: &AppState, stored: &Connector) -> Result<(), ApiError> {
    let id = stored.github_credential_id.ok_or(ApiError::NotFound)?;
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    github_credential::delete(&mut connection, id).await?;
    state
        .events
        .publish(&ServerEvent::GithubCredentialDeleted { id });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::checks::{CheckState, run_in_order};
    use crate::github::fake::{FakeGithub, FakeIssue, TOKEN};

    fn checks(fake: &FakeGithub, token: &str) -> GithubChecks {
        GithubChecks {
            github: fake.github.clone(),
            token: SecretString::from(token),
            scopes: Vec::new(),
            repositories: Vec::new(),
            sample: None,
        }
    }

    fn outcome_of(outcomes: &[(&'static str, Outcome)], id: &str) -> Outcome {
        outcomes
            .iter()
            .find(|(check, _outcome)| *check == id)
            .map(|(_check, outcome)| outcome.clone())
            .expect("the check ran")
    }

    #[tokio::test]
    async fn a_token_with_a_repository_counts_its_issues_and_pull_requests_apart() {
        let fake = FakeGithub::start().await;
        fake.put_issue("acme/app", 1, FakeIssue::open("A bug"));
        fake.put_issue("acme/app", 2, FakeIssue::open("Another bug"));
        fake.put_issue("acme/app", 3, FakeIssue::pull_request("The fix"));
        let mut checks = checks(&fake, TOKEN);
        let outcomes = run_in_order(DEFINITION.checks, &mut checks, |_id, _outcome| {}).await;

        assert_eq!(outcome_of(&outcomes, "sign_in").state, CheckState::Passed);
        // GitHub reports no scopes for a fine-grained token, which is how the fake answers.
        assert_eq!(
            outcome_of(&outcomes, "read_scopes").reason,
            Some("fine_grained_scopes")
        );
        assert_eq!(outcome_of(&outcomes, "list_repositories").count, Some(1));
        assert_eq!(outcome_of(&outcomes, "sample_issues").count, Some(2));
        assert_eq!(outcome_of(&outcomes, "sample_pull_requests").count, Some(1));
    }

    #[tokio::test]
    async fn a_token_reaching_no_repository_skips_both_samples() {
        let fake = FakeGithub::start().await;
        let mut checks = checks(&fake, TOKEN);
        let outcomes = run_in_order(DEFINITION.checks, &mut checks, |_id, _outcome| {}).await;

        for id in ["sample_issues", "sample_pull_requests"] {
            assert_eq!(
                outcome_of(&outcomes, id).reason,
                Some("no_repositories"),
                "{id}"
            );
        }
    }

    #[tokio::test]
    async fn a_token_github_refuses_fails_sign_in_and_skips_the_rest() {
        let fake = FakeGithub::start().await;
        let mut checks = checks(&fake, "ghp_wrong");
        let outcomes = run_in_order(DEFINITION.checks, &mut checks, |_id, _outcome| {}).await;

        assert_eq!(outcomes[0].1.state, CheckState::Failed);
        assert!(
            outcomes[1..]
                .iter()
                .all(|(_id, outcome)| outcome.state == CheckState::Skipped)
        );
        assert_eq!(
            outcome_of(&outcomes, "list_repositories").blocked_by,
            Some("sign_in")
        );
    }
}
