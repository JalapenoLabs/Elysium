// Copyright © 2026 Jalapeno Labs

//! Jira: issues on a Jira Cloud site, read through an Atlassian account by the link watcher.
//! See `docs/jira.md`.
//!
//! The connector's content is its account's allowlist, the projects and boards every call
//! made with that token is bounded by, which stays on the account
//! (`jira_credential_projects`, `jira_credential_boards`): every route, link, and watcher pass
//! that uses the token reads it there. So an Atlassian account carries exactly one Jira
//! connector, made with it, and a new Jira connector is always a new account.

use secrecy::SecretString;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{
    AccountChoice, AccountChoices, AccountDefinition, AccountField, AccountFieldType, ContentField,
    ContentFieldType, Definition, Source, parse_content, parse_new_account,
};
use crate::auth::CurrentUser;
use crate::connectors::checks::{CheckSpec, Checks, Outcome};
use crate::connectors::{AccountKind, ConnectorKind, Scope};
use crate::errors::ApiError;
use crate::jira::{Board, Jira, Project, Site, normalize_site_url};
use crate::models::connector::{self, AccountRef, Connector};
use crate::models::jira_credential::{
    self, Allowed, AllowedBoard, AllowedProject, Allowlist, JiraCredentialChanges,
};
use crate::realtime::ServerEvent;
use crate::routes::v1::jira_credentials::create_jira_credential::{self, RequestBody};
use crate::routes::v1::jira_credentials::{
    JiraCredentialResponse, Selection, allowlist, open, reach,
};
use crate::state::AppState;

/// The conditions a sample search runs under. Jira Cloud refuses a search with no condition
/// at all, so it asks for recently updated issues, as the issue pickers do.
const SAMPLE_JQL: &str = "updated >= -90d";

/// How many issues a sample reads: enough to prove the search works, few enough to be quick.
const SAMPLE_SIZE: u32 = 5;

pub const DEFINITION: Definition = Definition {
    kind: ConnectorKind::Jira,
    scope: Scope::Workspace,
    account: AccountKind::Atlassian,
    accounts: AccountChoices::New,
    start_url: None,
    content: &[
        ContentField {
            id: "projects",
            field: ContentFieldType::Selection,
            required: true,
        },
        ContentField {
            id: "boards",
            field: ContentFieldType::Selection,
            required: true,
        },
    ],
    checks: &[
        CheckSpec {
            id: "sign_in",
            required: true,
            depends_on: &[],
        },
        CheckSpec {
            id: "list_projects",
            required: true,
            depends_on: &["sign_in"],
        },
        CheckSpec {
            id: "list_boards",
            required: false,
            depends_on: &["sign_in"],
        },
        CheckSpec {
            id: "allowlist_reachable",
            required: true,
            depends_on: &["list_projects"],
        },
        CheckSpec {
            id: "sample_issues",
            required: true,
            depends_on: &["allowlist_reachable"],
        },
    ],
};

pub const ACCOUNT: AccountDefinition = AccountDefinition {
    kind: AccountKind::Atlassian,
    fields: &[
        AccountField {
            id: "name",
            field: AccountFieldType::Text,
            required: true,
        },
        AccountField {
            id: "siteUrl",
            field: AccountFieldType::Url,
            required: true,
        },
        AccountField {
            id: "accountEmail",
            field: AccountFieldType::Email,
            required: true,
        },
        AccountField {
            id: "token",
            field: AccountFieldType::Secret,
            required: true,
        },
    ],
};

/// A new Atlassian account's sign-in, for checks and options before anything is stored.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewSite {
    site_url: String,
    account_email: String,
    token: crate::routes::v1::jira_credentials::JiraToken,
}

/// The projects and boards a Jira connector reads.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Content {
    projects: Selection<String>,
    boards: Selection<i64>,
}

/// A site to call, signed in as the account.
#[derive(Debug)]
struct SignedSite {
    url: String,
    email: String,
    token: SecretString,
}

impl SignedSite {
    fn site(&self) -> Site<'_> {
        Site {
            url: &self.url,
            email: &self.email,
            token: &self.token,
        }
    }
}

/// The site a new account's fields name, normalized as a stored one is.
fn new_site(fields: Value) -> Result<SignedSite, ApiError> {
    let fields: NewSite = parse_new_account(AccountKind::Atlassian, fields)?;
    let url = normalize_site_url(&fields.site_url)
        .map_err(|advice| ApiError::BadRequest(advice.to_owned()))?;
    Ok(SignedSite {
        url,
        email: fields.account_email.trim().to_owned(),
        token: fields.token.0,
    })
}

/// A stored account's site and its allowlist.
async fn stored_site(state: &AppState, id: uuid::Uuid) -> Result<(SignedSite, Allowed), ApiError> {
    let opened = open(state, id).await?;
    let site = opened.site();
    let signed = SignedSite {
        url: site.url.to_owned(),
        email: site.email.to_owned(),
        token: site.token.clone(),
    };
    Ok((signed, opened.allowed))
}

/// What a selection picked, as the ids a listing would carry.
fn as_selection<Item, Reference>(
    allowlist: &Allowlist<Item>,
    reference: impl Fn(&Item) -> Reference,
) -> Selection<Reference> {
    match allowlist {
        Allowlist::All => Selection::All,
        Allowlist::Only(items) => Selection::Only(items.iter().map(reference).collect()),
    }
}

pub async fn prepare_checks(state: &AppState, source: Source<'_>) -> Result<JiraChecks, ApiError> {
    let (site, projects, boards) = match source {
        Source::Stored(stored) => {
            let id = stored.jira_credential_id.ok_or_else(|| {
                ApiError::BadRequest(
                    "a Jira connector reads through an Atlassian account".to_owned(),
                )
            })?;
            let (site, allowed) = stored_site(state, id).await?;
            let projects = as_selection(&allowed.projects, |project: &AllowedProject| {
                project.id.clone()
            });
            let boards = as_selection(&allowed.boards, |board: &AllowedBoard| board.id);
            (site, projects, boards)
        }
        Source::Draft { account, content } => {
            let content: Content = parse_content(ConnectorKind::Jira, content)?;
            let site = match account {
                AccountChoice::New { new } => new_site(new)?,
                AccountChoice::Existing { id } => stored_site(state, id).await?.0,
            };
            (site, content.projects, content.boards)
        }
    };
    Ok(JiraChecks {
        jira: state.jira.clone(),
        site,
        projects,
        boards,
        listed_projects: Vec::new(),
        listed_boards: None,
        allowed_projects: None,
    })
}

/// Jira's checks against one site and selection.
#[derive(Debug)]
pub struct JiraChecks {
    jira: Jira,
    site: SignedSite,
    projects: Selection<String>,
    boards: Selection<i64>,
    listed_projects: Vec<Project>,
    /// `None` until the boards are listed, which is optional.
    listed_boards: Option<Vec<Board>>,
    /// The projects the selection resolves to, once `allowlist_reachable` passes.
    allowed_projects: Option<Allowlist<AllowedProject>>,
}

impl Checks for JiraChecks {
    async fn check(&mut self, id: &'static str) -> Outcome {
        match id {
            "sign_in" => match self.jira.verify(&self.site.site()).await {
                Ok(_account) => Outcome::passed(),
                Err(error) => Outcome::failed(error.to_string()),
            },
            "list_projects" => match self.jira.list_projects(&self.site.site()).await {
                Ok(listing) => {
                    self.listed_projects = listing.items;
                    Outcome::counted(self.listed_projects.len())
                }
                Err(error) => Outcome::failed(error.to_string()),
            },
            "list_boards" => match self.jira.list_boards(&self.site.site()).await {
                Ok(listing) => {
                    let count = listing.items.len();
                    self.listed_boards = Some(listing.items);
                    Outcome::counted(count)
                }
                Err(error) => Outcome::failed(error.to_string()),
            },
            "allowlist_reachable" => self.allowlist_reachable(),
            "sample_issues" => self.sample_issues().await,
            unknown => Outcome::failed(format!("Jira has no check {unknown}")),
        }
    }
}

impl JiraChecks {
    /// Every project and board the selection names is one the token reaches now.
    fn allowlist_reachable(&mut self) -> Outcome {
        let mut missing: Vec<String> = Vec::new();
        let projects = match &self.projects {
            Selection::All => Allowlist::All,
            Selection::Only(references) => {
                let mut chosen: Vec<AllowedProject> = Vec::with_capacity(references.len());
                for reference in references {
                    match self.listed_projects.iter().find(|project| {
                        project.id == *reference || project.key.eq_ignore_ascii_case(reference)
                    }) {
                        Some(project) => chosen.push(AllowedProject {
                            id: project.id.clone(),
                            key: project.key.clone(),
                            name: project.name.clone(),
                        }),
                        None => missing.push(reference.clone()),
                    }
                }
                Allowlist::Only(chosen)
            }
        };
        if let (Selection::Only(references), Some(boards)) = (&self.boards, &self.listed_boards) {
            for reference in references {
                if !boards.iter().any(|board| board.id == *reference) {
                    missing.push(format!("board {reference}"));
                }
            }
        }
        if !missing.is_empty() {
            return Outcome::failed(format!(
                "the token no longer reaches {}",
                missing.join(", ")
            ));
        }
        let count = match &projects {
            Allowlist::All => self.listed_projects.len(),
            Allowlist::Only(chosen) => chosen.len(),
        };
        self.allowed_projects = Some(projects);
        Outcome::counted(count)
    }

    /// A bounded search over the allowed projects answers.
    async fn sample_issues(&self) -> Outcome {
        let Some(projects) = &self.allowed_projects else {
            return Outcome::failed_because("allowlist_unresolved");
        };
        let jql = match allowlist::bounded_jql(SAMPLE_JQL, projects) {
            Ok(Some(jql)) => jql,
            Ok(None) => return Outcome::skipped("no_projects_allowed"),
            Err(error) => return Outcome::failed(error.to_string()),
        };
        match self
            .jira
            .search(&self.site.site(), &jql, SAMPLE_SIZE, None)
            .await
        {
            Ok(search) => Outcome::counted(search.issues.len()),
            Err(error) => Outcome::failed(error.to_string()),
        }
    }
}

/// A Jira connector comes with a new Atlassian account, made the way Settings, Jira makes one.
///
/// # Errors
/// [`ApiError::CodedConflict`] for an existing account, which already carries its connector;
/// otherwise as [`create_jira_credential::create_credential`].
pub async fn create(
    state: &AppState,
    current: &CurrentUser,
    account: AccountChoice,
    content: Value,
) -> Result<Connector, ApiError> {
    let AccountChoice::New { new } = account else {
        return Err(ApiError::CodedConflict {
            code: "account_has_connector",
            message: "an Atlassian account already carries its Jira connector",
        });
    };
    let Value::Object(mut body) = new else {
        return Err(ApiError::BadRequest(
            "a new Atlassian account is an object of its fields".to_owned(),
        ));
    };
    let Value::Object(content) = content else {
        return Err(ApiError::BadRequest(
            "jira content is an object of projects and boards".to_owned(),
        ));
    };
    body.extend(content);
    let body: RequestBody = serde_json::from_value(Value::Object(body)).map_err(|error| {
        ApiError::BadRequest(format!("a new Jira connector is not valid: {error}"))
    })?;
    let (credential, _allowed) =
        create_jira_credential::create_credential(state, current.id(), body).await?;

    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let found = connector::find_by_account(
        &mut connection,
        ConnectorKind::Jira.as_str(),
        AccountRef::Atlassian(credential.id),
    )
    .await?;
    found.ok_or(ApiError::NotFound)
}

/// What the projects and boards can be picked from, as the account's token reaches them now.
///
/// # Errors
/// As [`reach`].
pub async fn options(state: &AppState, account: AccountChoice) -> Result<Value, ApiError> {
    let site = match account {
        AccountChoice::New { new } => new_site(new)?,
        AccountChoice::Existing { id } => stored_site(state, id).await?.0,
    };
    let reachable = reach(&state.jira, &site.site()).await?;
    let projects: Vec<Value> = reachable
        .projects
        .iter()
        .map(|project| json!({ "id": project.id, "label": project.name, "detail": project.key }))
        .collect();
    let boards: Vec<Value> = reachable
        .boards
        .iter()
        .map(|board| json!({ "id": board.id, "label": board.name, "detail": board.project_key }))
        .collect();
    let mut fields = Map::new();
    fields.insert(
        "projects".to_owned(),
        json!({ "options": projects, "truncated": reachable.projects_truncated }),
    );
    fields.insert(
        "boards".to_owned(),
        json!({ "options": boards, "truncated": reachable.boards_truncated }),
    );
    Ok(Value::Object(fields))
}

/// Replaces the projects and boards, re-checked with Jira first as every allowlist change is.
///
/// # Errors
/// [`ApiError::BadRequest`] for a pick Jira does not report, and whatever Jira answered.
pub async fn update_content(
    state: &AppState,
    stored: &Connector,
    content: Value,
) -> Result<Connector, ApiError> {
    let content: Content = parse_content(ConnectorKind::Jira, content)?;
    let id = stored.jira_credential_id.ok_or_else(|| {
        ApiError::BadRequest("a Jira connector reads through an Atlassian account".to_owned())
    })?;
    let (site, _allowed) = stored_site(state, id).await?;
    let reachable = reach(&state.jira, &site.site()).await?;
    let changes = JiraCredentialChanges {
        projects: Some(reachable.choose_projects(&content.projects)?),
        boards: Some(reachable.choose_boards(&content.boards)?),
        ..JiraCredentialChanges::default()
    };

    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let credential = jira_credential::update(&mut connection, &state.cipher, id, &changes).await?;
    let allowed = jira_credential::allowed_of(&mut connection, &credential).await?;
    state.events.publish(&ServerEvent::JiraCredentialUpserted(
        JiraCredentialResponse::new(credential, allowed),
    ));
    state.connectors.activity.changed(stored.id);
    Ok(connector::find(&mut connection, stored.id).await?)
}

/// Removes the connector with its Atlassian account, as Settings, Jira removes one.
///
/// # Errors
/// [`ApiError::NotFound`] when the account was already removed.
pub async fn remove(state: &AppState, stored: &Connector) -> Result<(), ApiError> {
    let id = stored.jira_credential_id.ok_or(ApiError::NotFound)?;
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    jira_credential::delete(&mut connection, id).await?;
    state
        .events
        .publish(&ServerEvent::JiraCredentialDeleted { id });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::checks::{CheckState, run_in_order};
    use crate::jira::tests::{EMAIL, SITE_URL, TOKEN, fake_jira_site};

    async fn checks(token: &str, projects: Selection<String>) -> JiraChecks {
        let (jira, _log) = fake_jira_site().await;
        JiraChecks {
            jira,
            site: SignedSite {
                url: SITE_URL.to_owned(),
                email: EMAIL.to_owned(),
                token: SecretString::from(token),
            },
            projects,
            boards: Selection::All,
            listed_projects: Vec::new(),
            listed_boards: None,
            allowed_projects: None,
        }
    }

    fn states(outcomes: &[(&'static str, Outcome)]) -> Vec<(&'static str, CheckState)> {
        outcomes
            .iter()
            .map(|(id, outcome)| (*id, outcome.state))
            .collect()
    }

    #[tokio::test]
    async fn a_site_whose_allowlist_it_reaches_passes_every_check() {
        let mut checks = checks(TOKEN, Selection::Only(vec!["ELY".to_owned()])).await;
        let outcomes = run_in_order(DEFINITION.checks, &mut checks, |_id, _outcome| {}).await;

        assert!(
            outcomes
                .iter()
                .all(|(_id, outcome)| outcome.state == CheckState::Passed),
            "{:?}",
            states(&outcomes)
        );
        assert_eq!(outcomes[3].1.count, Some(1), "one project is allowed");
    }

    #[tokio::test]
    async fn a_project_the_token_no_longer_reaches_fails_the_allowlist_and_skips_the_sample() {
        let mut checks = checks(TOKEN, Selection::Only(vec!["GONE".to_owned()])).await;
        let outcomes = run_in_order(DEFINITION.checks, &mut checks, |_id, _outcome| {}).await;

        let (_id, allowlist) = &outcomes[3];
        assert_eq!(allowlist.state, CheckState::Failed);
        assert!(
            allowlist
                .message
                .as_deref()
                .is_some_and(|message| message.contains("GONE"))
        );
        let (_id, sample) = &outcomes[4];
        assert_eq!(sample.state, CheckState::Skipped);
        assert_eq!(sample.blocked_by, Some("allowlist_reachable"));
    }

    #[tokio::test]
    async fn a_token_jira_refuses_fails_sign_in_and_nothing_after_it_runs() {
        let mut checks = checks("not-the-token", Selection::All).await;
        let outcomes = run_in_order(DEFINITION.checks, &mut checks, |_id, _outcome| {}).await;

        assert_eq!(outcomes[0].1.state, CheckState::Failed);
        assert!(
            outcomes[1..]
                .iter()
                .all(|(_id, outcome)| outcome.state == CheckState::Skipped),
            "{:?}",
            states(&outcomes)
        );
    }
}
