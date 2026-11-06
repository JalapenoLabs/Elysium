// Copyright © 2026 Jalapeno Labs

//! GitHub: issues and pull requests, read through a GitHub account by the link watcher. See
//! `docs/github.md`.
//!
//! Its content is the repositories whose issues assigned to the token's account come in as
//! items (`crate::action_items::assigned`): every one the token reaches (`"*"`), or the ones
//! picked from what it lists, stored as GitHub spells them. Links are not bounded by it: a link
//! can be made to anything the token reaches. A GitHub account carries exactly one GitHub
//! connector, made with it, since links can be made through any token and the watcher reads
//! through each; a new GitHub connector is always a new account.

use secrecy::SecretString;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{
    AccountChoice, AccountDefinition, AccountField, AccountFieldType, ContentField,
    ContentFieldType, Definition, Provides, Replies, Source, parse_content,
};
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
use crate::routes::v1::jira_credentials::Selection;
use crate::state::AppState;

pub const DEFINITION: Definition = Definition {
    kind: ConnectorKind::Github,
    scope: Scope::Workspace,
    account: AccountKind::Github,
    provides: &[Provides::Issues],
    start_url: None,
    content: &[ContentField {
        id: "repositories",
        field: ContentFieldType::Selection,
        required: true,
    }],
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
    replies: Replies::Never,
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

/// The repositories a GitHub connector brings assigned issues in from, `owner/name` as GitHub
/// spells them.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Content {
    pub repositories: Selection<String>,
}

/// `selection` as a connector stores it, each pick matched against `reachable`, what the token
/// lists, in any case, and kept as GitHub spells it.
///
/// # Errors
/// [`ApiError::BadRequest`] naming a pick the token does not reach, or for picking nothing.
pub fn stored_content(
    selection: &Selection<String>,
    reachable: &[ListedRepository],
) -> Result<Value, ApiError> {
    let Selection::Only(picked) = selection else {
        return Ok(json!({ "repositories": "*" }));
    };
    let mut names: Vec<&str> = Vec::with_capacity(picked.len());
    let mut missing: Vec<&str> = Vec::new();
    for pick in picked {
        let found = reachable
            .iter()
            .find(|repository| repository.full_name.eq_ignore_ascii_case(pick.trim()));
        match found {
            Some(repository) => names.push(&repository.full_name),
            None => missing.push(pick),
        }
    }
    if !missing.is_empty() {
        return Err(ApiError::BadRequest(format!(
            "the token does not reach {}",
            missing.join(", ")
        )));
    }
    if names.is_empty() {
        return Err(ApiError::BadRequest(
            "pick at least one repository, or every repository".to_owned(),
        ));
    }
    Ok(json!({ "repositories": names }))
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

/// A GitHub connector comes with its new GitHub account, `new`, made the way Settings, GitHub
/// makes one, reading the repositories `content` picks.
///
/// # Errors
/// As [`create_github_credential::create_credential`], and [`ApiError::BadRequest`] for content
/// that is not a selection of repositories.
pub async fn create(
    state: &AppState,
    current: &CurrentUser,
    new: Value,
    content: Value,
) -> Result<Connector, ApiError> {
    let content: Content = parse_content(ConnectorKind::Github, content)?;
    let body: RequestBody = serde_json::from_value(new).map_err(|error| {
        ApiError::BadRequest(format!("a new GitHub account is not valid: {error}"))
    })?;
    let credential = create_github_credential::create_credential(
        state,
        current.id(),
        body,
        &content.repositories,
    )
    .await?;

    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let found =
        connector::find_by_account(&mut connection, AccountRef::Github(credential.id)).await?;
    found.ok_or(ApiError::NotFound)
}

/// What `repositories` can pick from through `account`: every repository its token lists.
///
/// # Errors
/// [`ApiError::BadRequest`] for a new account that is not valid, [`ApiError::NotFound`] for an
/// account that does not exist, and whatever GitHub answered.
pub async fn options(state: &AppState, account: AccountChoice) -> Result<Value, ApiError> {
    let token = match account {
        AccountChoice::Existing { id } => stored_token(state, id).await?,
        AccountChoice::New { new } => {
            let fields: NewToken = super::parse_new_account(AccountKind::Github, new)?;
            fields.token.0
        }
    };
    let listing = state.github.list_repositories(&token).await?;
    let options: Vec<Value> = listing
        .repositories
        .iter()
        .map(|repository| json!({ "id": repository.full_name, "label": repository.full_name }))
        .collect();
    let mut fields = Map::new();
    fields.insert(
        "repositories".to_owned(),
        json!({ "options": options, "truncated": listing.truncated }),
    );
    Ok(Value::Object(fields))
}

/// Replaces the repositories it brings assigned issues in from, checked against what the token
/// lists now.
///
/// # Errors
/// [`ApiError::BadRequest`] for content that is not a selection or names a repository the token
/// does not reach, and whatever GitHub answered.
pub async fn update_content(
    state: &AppState,
    stored: &Connector,
    content: Value,
) -> Result<Connector, ApiError> {
    let content: Content = parse_content(ConnectorKind::Github, content)?;
    let id = stored.github_credential_id.ok_or_else(|| {
        ApiError::BadRequest("a GitHub connector reads through a GitHub account".to_owned())
    })?;
    let reachable = match &content.repositories {
        Selection::All => Vec::new(),
        Selection::Only(_picked) => {
            let token = stored_token(state, id).await?;
            state.github.list_repositories(&token).await?.repositories
        }
    };
    let stored_value = stored_content(&content.repositories, &reachable)?;
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let updated = connector::set_content(
        &mut connection,
        stored.id,
        stored_value,
        stored.storage_location_id,
    )
    .await?;
    state.connectors.activity.changed(stored.id);
    Ok(updated)
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

    fn listed(full_name: &str) -> ListedRepository {
        let (owner, name) = full_name.split_once('/').expect("owner/name");
        ListedRepository {
            full_name: full_name.to_owned(),
            owner: owner.to_owned(),
            name: name.to_owned(),
            is_private: false,
            is_archived: false,
            default_branch: "main".to_owned(),
            clone_url: format!("https://github.com/{full_name}.git"),
            pushed_at: None,
            role_can_push: true,
        }
    }

    #[test]
    fn repositories_are_stored_as_github_spells_them_and_only_if_the_token_reaches_them() {
        let reachable = [
            listed("JalapenoLabs/Elysium"),
            listed("JalapenoLabs/Seraphim"),
        ];

        let everything = stored_content(&Selection::All, &[]).expect("everything");
        assert_eq!(everything, json!({ "repositories": "*" }));

        let picked = Selection::Only(vec!["jalapenolabs/elysium".to_owned()]);
        assert_eq!(
            stored_content(&picked, &reachable).expect("picked"),
            json!({ "repositories": ["JalapenoLabs/Elysium"] })
        );

        let unreachable = Selection::Only(vec!["acme/secret".to_owned()]);
        let refused = stored_content(&unreachable, &reachable).expect_err("refused");
        assert!(refused.to_string().contains("acme/secret"), "{refused}");

        stored_content(&Selection::Only(Vec::new()), &reachable).expect_err("nothing picked");
    }

    #[test]
    fn content_names_its_repositories_or_everything() {
        let everything: Content =
            serde_json::from_value(json!({ "repositories": "*" })).expect("parses");
        assert_eq!(everything.repositories, Selection::All);
        serde_json::from_value::<Content>(json!({})).expect_err("a choice is required");
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
