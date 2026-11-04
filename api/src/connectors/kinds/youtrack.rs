// Copyright © 2026 Jalapeno Labs

//! YouTrack: issues and comments on a YouTrack Cloud site, read with a person's own permanent
//! token. See `docs/connectors.md`.
//!
//! The account is the token and the site it was issued on, a token account the connector owns
//! (`token_accounts`, with `site_url`), given with it and removed with it. The connector's
//! content is the projects it reads,
//! picked from what the token reaches, or all of them (`"*"`), stored as YouTrack's project
//! ids in `content`. The reader in `crate::youtrack::reader` reads it.

use diesel_async::AsyncConnection;
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::{
    AccountChoice, AccountDefinition, AccountField, AccountFieldType, ContentField,
    ContentFieldType, Definition, Provides, Replies, Source, parse_content, parse_new_account,
};
use crate::auth::CurrentUser;
use crate::connectors::checks::{CheckSpec, Checks, Outcome};
use crate::connectors::{AccountKind, ConnectorKind, Scope};
use crate::errors::ApiError;
use crate::models::connector::{self, AccountRef, Connector, NewConnector};
use crate::models::token_account::{self, NewTokenAccount};
use crate::routes::v1::connectors::ApiKey;
use crate::routes::v1::jira_credentials::Selection;
use crate::state::AppState;
use crate::youtrack::{Issue, Project, Site, Youtrack, YoutrackError, normalize_site_url};

/// How many issues the sample check reads: enough to prove the search works, few enough to be
/// quick.
const SAMPLE_SIZE: u32 = 5;

/// How many comments the comment check reads from the newest issue.
const SAMPLE_COMMENTS: u32 = 5;

/// What an account is called when its person names none.
const DEFAULT_NAME: &str = "YouTrack";

pub const DEFINITION: Definition = Definition {
    kind: ConnectorKind::Youtrack,
    scope: Scope::Personal,
    account: AccountKind::Youtrack,
    provides: &[Provides::Issues],
    start_url: None,
    content: &[ContentField {
        id: "projects",
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
            id: "list_projects",
            required: true,
            depends_on: &["sign_in"],
        },
        CheckSpec {
            id: "sample_issues",
            required: true,
            depends_on: &["list_projects"],
        },
        CheckSpec {
            id: "sample_comments",
            required: false,
            depends_on: &["sample_issues"],
        },
    ],
    replies: Replies::Never,
};

pub const ACCOUNT: AccountDefinition = AccountDefinition {
    kind: AccountKind::Youtrack,
    fields: &[
        AccountField {
            id: "name",
            field: AccountFieldType::Text,
            required: false,
        },
        AccountField {
            id: "siteUrl",
            field: AccountFieldType::Url,
            required: true,
        },
        AccountField {
            id: "token",
            field: AccountFieldType::Secret,
            required: true,
        },
    ],
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NewAccount {
    #[serde(default)]
    name: Option<String>,
    site_url: String,
    token: ApiKey,
}

/// The projects a YouTrack connector reads, by YouTrack id or short name.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Content {
    pub projects: Selection<String>,
}

/// A site and the token it signs in with.
#[derive(Debug)]
struct SignedSite {
    url: String,
    token: SecretString,
}

impl SignedSite {
    fn site(&self) -> Site<'_> {
        Site {
            url: &self.url,
            token: &self.token,
        }
    }
}

/// The search a selection reads, over the projects the token reaches now: no condition for
/// every project, or `project: JL, WAY`. `None` when it names no project the token reaches.
pub fn project_query(selection: &Selection<String>, reachable: &[Project]) -> Option<String> {
    let Selection::Only(references) = selection else {
        return Some(String::new());
    };
    let named: Vec<&str> = reachable
        .iter()
        .filter(|project| references.iter().any(|reference| names(project, reference)))
        .map(|project| project.short_name.as_str())
        .collect();
    (!named.is_empty()).then(|| format!("project: {}", named.join(", ")))
}

/// Whether `reference` names `project`, by its id or its short name in any case.
fn names(project: &Project, reference: &str) -> bool {
    project.id == reference || project.short_name.eq_ignore_ascii_case(reference)
}

/// A YouTrack failure as the person setting up a connector should see it.
fn api_error(error: &YoutrackError) -> ApiError {
    match error {
        YoutrackError::Unauthorized(message) => ApiError::BadRequest((*message).to_owned()),
        YoutrackError::NotFound | YoutrackError::Refused(_) => {
            ApiError::BadGateway(error.to_string())
        }
    }
}

/// `owner`'s YouTrack account `id`, signed in.
async fn owned_site(state: &AppState, id: Uuid, owner: Uuid) -> Result<SignedSite, ApiError> {
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let account = token_account::find_owned(&mut connection, id, owner).await?;
    if account.kind != AccountKind::Youtrack.as_str() {
        return Err(ApiError::BadRequest(
            "that account's key is not a YouTrack token".to_owned(),
        ));
    }
    let url = account.site_url.clone().ok_or_else(|| {
        anyhow::anyhow!("a YouTrack account names its site, and this one has none")
    })?;
    let token = account
        .credential(&state.cipher)
        .map_err(|_open_error| anyhow::anyhow!("the stored token cannot be decrypted"))?;
    Ok(SignedSite { url, token })
}

/// A new account's fields, with its site normalized as a stored one is.
fn new_site(fields: Value) -> Result<(Option<String>, SignedSite), ApiError> {
    let fields: NewAccount = parse_new_account(AccountKind::Youtrack, fields)?;
    let url = normalize_site_url(&fields.site_url)
        .map_err(|advice| ApiError::BadRequest(advice.to_owned()))?;
    Ok((
        fields.name,
        SignedSite {
            url,
            token: fields.token.0,
        },
    ))
}

/// The site an account choice signs in to, for `current`.
async fn chosen_site(
    state: &AppState,
    current: &CurrentUser,
    account: AccountChoice,
) -> Result<SignedSite, ApiError> {
    match account {
        AccountChoice::Existing { id } => owned_site(state, id, current.id()).await,
        AccountChoice::New { new } => Ok(new_site(new)?.1),
    }
}

pub async fn prepare_checks(
    state: &AppState,
    current: &CurrentUser,
    source: Source<'_>,
) -> Result<YoutrackChecks, ApiError> {
    let (site, content) = match source {
        Source::Stored(stored) => {
            let id = stored.token_account_id.ok_or_else(|| {
                ApiError::BadRequest("a YouTrack connector reads with a token".to_owned())
            })?;
            let site = owned_site(state, id, stored.created_by).await?;
            (site, stored.content.clone())
        }
        Source::Draft { account, content } => {
            (chosen_site(state, current, account).await?, content)
        }
    };
    let content: Content = parse_content(ConnectorKind::Youtrack, content)?;
    Ok(YoutrackChecks {
        youtrack: state.connectors.youtrack.youtrack().clone(),
        site,
        projects: content.projects,
        reachable: Vec::new(),
        sampled: Vec::new(),
    })
}

/// YouTrack's checks against one site and selection.
pub struct YoutrackChecks {
    youtrack: Youtrack,
    site: SignedSite,
    projects: Selection<String>,
    reachable: Vec<Project>,
    sampled: Vec<Issue>,
}

impl std::fmt::Debug for YoutrackChecks {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("YoutrackChecks")
            .finish_non_exhaustive()
    }
}

impl Checks for YoutrackChecks {
    async fn check(&mut self, id: &'static str) -> Outcome {
        match id {
            "sign_in" => match self.youtrack.whoami(&self.site.site()).await {
                Ok(user) => Outcome {
                    message: Some(user.login),
                    ..Outcome::passed()
                },
                Err(error) => Outcome::failed(error.to_string()),
            },
            "list_projects" => match self.youtrack.list_projects(&self.site.site()).await {
                Ok(projects) => {
                    self.reachable = projects.items;
                    Outcome::counted(self.reachable.len())
                }
                Err(error) => Outcome::failed(error.to_string()),
            },
            "sample_issues" => self.sample_issues().await,
            "sample_comments" => self.sample_comments().await,
            unknown => Outcome::failed(format!("YouTrack has no check {unknown}")),
        }
    }
}

impl YoutrackChecks {
    /// A search over the chosen projects answers.
    async fn sample_issues(&mut self) -> Outcome {
        let Some(query) = project_query(&self.projects, &self.reachable) else {
            return Outcome::failed_because("projects_unreachable");
        };
        match self
            .youtrack
            .search(&self.site.site(), &query, 0, SAMPLE_SIZE)
            .await
        {
            Ok(issues) => {
                self.sampled = issues;
                Outcome::counted(self.sampled.len())
            }
            Err(error) => Outcome::failed(error.to_string()),
        }
    }

    /// The newest issue's comments can be read.
    async fn sample_comments(&self) -> Outcome {
        let Some(issue) = self.sampled.first() else {
            return Outcome::skipped("no_issues");
        };
        match self
            .youtrack
            .comments(&self.site.site(), &issue.id, SAMPLE_COMMENTS)
            .await
        {
            Ok(comments) => Outcome {
                message: Some(issue.key.clone()),
                ..Outcome::counted(comments.len())
            },
            Err(error) => Outcome::failed(error.to_string()),
        }
    }
}

/// `selection` as it is stored: `"*"`, or the ids of the reachable projects it names.
///
/// # Errors
/// [`ApiError::BadRequest`] naming every project the token does not reach.
fn stored_content(selection: &Selection<String>, reachable: &[Project]) -> Result<Value, ApiError> {
    let Selection::Only(references) = selection else {
        return Ok(json!({ "projects": "*" }));
    };
    let mut ids: Vec<&str> = Vec::with_capacity(references.len());
    let mut missing: Vec<&str> = Vec::new();
    for reference in references {
        match reachable.iter().find(|project| names(project, reference)) {
            Some(project) => ids.push(&project.id),
            None => missing.push(reference),
        }
    }
    if !missing.is_empty() {
        return Err(ApiError::BadRequest(format!(
            "the token does not reach {}",
            missing.join(", ")
        )));
    }
    if ids.is_empty() {
        return Err(ApiError::BadRequest(
            "pick at least one project, or every project".to_owned(),
        ));
    }
    Ok(json!({ "projects": ids }))
}

/// Connects `current`'s YouTrack with the new token `new`, checked with its site before it is
/// stored, reading the content's projects. The token's account and the connector are stored
/// together.
///
/// # Errors
/// [`ApiError::BadRequest`] for content, a project the token does not reach, or a token YouTrack
/// refuses; [`ApiError::BadGateway`] when the site cannot be reached.
pub async fn create(
    state: &AppState,
    current: &CurrentUser,
    new: Value,
    content: Value,
) -> Result<Connector, ApiError> {
    let content: Content = parse_content(ConnectorKind::Youtrack, content)?;
    let youtrack = state.connectors.youtrack.youtrack();
    let (name, site) = new_site(new)?;
    // A token is stored only once its site accepts it, so no account holds one nobody tried.
    let user = youtrack
        .whoami(&site.site())
        .await
        .map_err(|error| api_error(&error))?;
    let reachable = youtrack
        .list_projects(&site.site())
        .await
        .map_err(|error| api_error(&error))?;
    let projects = stored_content(&content.projects, &reachable.items)?;

    let name = name
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| DEFAULT_NAME.to_owned());
    let mut new_account = NewTokenAccount::new(
        &state.cipher,
        AccountKind::Youtrack.as_str(),
        &name.chars().take(120).collect::<String>(),
        &site.token,
        &user.login,
        current.id(),
    );
    new_account.site_url = Some(site.url.clone());

    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let created = connection
        .transaction(async |connection| {
            let account = token_account::create(connection, new_account).await?;
            let mut new_connector = NewConnector::new(
                ConnectorKind::Youtrack.as_str(),
                AccountRef::Token(account.id),
                current.id(),
            );
            new_connector.content = projects;
            connector::create(connection, new_connector).await
        })
        .await?;
    Ok(created)
}

/// What the projects can be picked from, as the account's token reaches them now.
///
/// # Errors
/// As [`create`].
pub async fn options(
    state: &AppState,
    current: &CurrentUser,
    account: AccountChoice,
) -> Result<Value, ApiError> {
    let site = chosen_site(state, current, account).await?;
    let projects = state
        .connectors
        .youtrack
        .youtrack()
        .list_projects(&site.site())
        .await
        .map_err(|error| api_error(&error))?;
    let options: Vec<Value> = projects
        .items
        .iter()
        .map(|project| json!({ "id": project.id, "label": project.name, "detail": project.short_name }))
        .collect();
    let mut fields = Map::new();
    fields.insert(
        "projects".to_owned(),
        json!({ "options": options, "truncated": projects.truncated }),
    );
    Ok(Value::Object(fields))
}

/// Replaces the projects it reads, checked against what the token reaches now.
///
/// # Errors
/// [`ApiError::BadRequest`] for a project the token does not reach, and whatever YouTrack
/// answered.
pub async fn update_content(
    state: &AppState,
    stored: &Connector,
    content: Value,
) -> Result<Connector, ApiError> {
    let content: Content = parse_content(ConnectorKind::Youtrack, content)?;
    let id = stored.token_account_id.ok_or_else(|| {
        ApiError::BadRequest("a YouTrack connector reads with a token".to_owned())
    })?;
    let site = owned_site(state, id, stored.created_by).await?;
    let reachable = state
        .connectors
        .youtrack
        .youtrack()
        .list_projects(&site.site())
        .await
        .map_err(|error| api_error(&error))?;
    let stored_value = stored_content(&content.projects, &reachable.items)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::checks::{CheckState, run_in_order};
    use crate::youtrack::tests::{
        FakeComment, FakeSite, SITE_URL, TOKEN, fake_youtrack_site, sample_issue,
    };

    fn project(id: &str, short_name: &str) -> Project {
        Project {
            id: id.to_owned(),
            short_name: short_name.to_owned(),
            name: short_name.to_owned(),
        }
    }

    async fn checks(token: &str, projects: Selection<String>, site: FakeSite) -> YoutrackChecks {
        let (youtrack, _shared) = fake_youtrack_site(site).await;
        YoutrackChecks {
            youtrack,
            site: SignedSite {
                url: SITE_URL.to_owned(),
                token: SecretString::from(token),
            },
            projects,
            reachable: Vec::new(),
            sampled: Vec::new(),
        }
    }

    #[test]
    fn a_selection_becomes_a_search_over_the_projects_it_still_reaches() {
        let reachable = [project("0-1", "JL"), project("0-2", "WAY")];

        assert_eq!(
            project_query(&Selection::All, &reachable),
            Some(String::new())
        );
        assert_eq!(
            project_query(
                &Selection::Only(vec!["0-2".to_owned(), "jl".to_owned(), "GONE".to_owned()]),
                &reachable
            ),
            Some("project: JL, WAY".to_owned())
        );
        assert_eq!(
            project_query(&Selection::Only(vec!["GONE".to_owned()]), &reachable),
            None
        );
    }

    #[test]
    fn content_is_stored_as_project_ids_and_refuses_what_the_token_does_not_reach() {
        let reachable = [project("0-1", "JL"), project("0-2", "WAY")];

        assert_eq!(
            stored_content(&Selection::Only(vec!["WAY".to_owned()]), &reachable).expect("stored"),
            json!({ "projects": ["0-2"] })
        );
        assert_eq!(
            stored_content(&Selection::All, &reachable).expect("stored"),
            json!({ "projects": "*" })
        );
        let refused =
            stored_content(&Selection::Only(vec!["SECRET".to_owned()]), &reachable).unwrap_err();
        assert!(matches!(refused, ApiError::BadRequest(message) if message.contains("SECRET")));
        assert!(matches!(
            stored_content(&Selection::Only(Vec::new()), &reachable),
            Err(ApiError::BadRequest(_))
        ));
    }

    #[tokio::test]
    async fn a_site_with_issues_and_comments_passes_every_check() {
        let site = FakeSite {
            issues: vec![sample_issue("2-1", "JL-1", 1_000)],
            comments: vec![FakeComment {
                issue_id: "2-1",
                id: "4-1",
                text: "First",
                created: 1_000,
                author: "bea",
            }],
            ..FakeSite::default()
        };
        let mut checks = checks(TOKEN, Selection::Only(vec!["JL".to_owned()]), site).await;
        let outcomes = run_in_order(DEFINITION.checks, &mut checks, |_id, _outcome| {}).await;

        let states: Vec<CheckState> = outcomes
            .iter()
            .map(|(_id, outcome)| outcome.state)
            .collect();
        assert_eq!(states, [CheckState::Passed; 4]);
        assert_eq!(
            outcomes[1].1.count,
            Some(2),
            "the archived project is left out"
        );
        assert_eq!(outcomes[3].1.count, Some(1));
    }

    #[tokio::test]
    async fn projects_the_token_no_longer_reaches_fail_the_sample_and_skip_comments() {
        let mut checks = checks(
            TOKEN,
            Selection::Only(vec!["GONE".to_owned()]),
            FakeSite::default(),
        )
        .await;
        let outcomes = run_in_order(DEFINITION.checks, &mut checks, |_id, _outcome| {}).await;

        assert_eq!(outcomes[2].1.state, CheckState::Failed);
        assert_eq!(outcomes[2].1.reason, Some("projects_unreachable"));
        assert_eq!(outcomes[3].1.state, CheckState::Skipped);
    }

    #[tokio::test]
    async fn a_token_the_site_refuses_fails_sign_in_and_nothing_after_it_runs() {
        let mut checks = checks("perm:wrong", Selection::All, FakeSite::default()).await;
        let outcomes = run_in_order(DEFINITION.checks, &mut checks, |_id, _outcome| {}).await;

        assert_eq!(outcomes[0].1.state, CheckState::Failed);
        assert!(
            outcomes[1..]
                .iter()
                .all(|(_id, outcome)| outcome.state == CheckState::Skipped)
        );
    }
}
