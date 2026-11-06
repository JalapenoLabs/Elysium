// Copyright © 2026 Jalapeno Labs

//! Links through a person's YouTrack token account: issues on items, and the unresolved issues
//! assigned to the token's user in the connector's chosen projects, which come in as items.
//! Initiatives contain no YouTrack issues yet, so a container is refused.
//!
//! Every call goes through `crate::youtrack` to the site the account names, signed in as the
//! token's user, so a link reaches no further than that person can.
//!
//! Filing an issue opens it in a project the account reaches, as the token's user; a filing
//! tried before looks first among the project's latest issues for one that user opened with
//! its title.
//!
//! An issue is done when YouTrack counts its state as resolved. Resolving an item moves its
//! issue to a resolved value of the project's state field (whatever the project calls it, such as
//! `State` or `Stage`; [`done_state`] picks which value), unless YouTrack already counts it
//! resolved.

use std::sync::Arc;

use anyhow::Context;
use chrono::{DateTime, TimeDelta, Utc};
use futures_util::FutureExt as _;
use futures_util::future::BoxFuture;
use secrecy::SecretString;
use serde_json::Value;
use tracing::{Level, event};
use uuid::Uuid;

use super::{
    AssignedIssues, Children, Destination, IssueToFile, LinkError, Provider, Remote,
    RemoteContainer, WriteOutcome,
};
use crate::connectors::AccountKind;
use crate::connectors::kinds::youtrack::{Content, project_query};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::action_item::Owner;
use crate::models::action_item_link::{ActionItemLink, LinkKind, LinkState};
use crate::models::initiative_link::{ContainerKind, InitiativeLink};
use crate::models::token_account;
use crate::youtrack::{
    Issue, Project, Site, StateField, Youtrack, YoutrackError, is_issue_key, issue_url,
};

/// How many issue keys one watcher search names. Keeps the query short; a credential with more
/// links is read in several searches.
const KEYS_PER_SEARCH: usize = 50;

/// Minutes reached back past the cursor, as the other providers do, so an issue updated in the
/// moment the cursor was taken, or under a small clock difference, is read again rather than
/// missed. Applying a change twice changes nothing.
const CURSOR_OVERLAP_MINUTES: i64 = 5;

/// How many of a project's latest issues a filing tried before looks through for the one its
/// earlier attempt opened. That issue is among the newest the project has, so a page is plenty.
const FILED_SEARCH_SIZE: u32 = 50;

/// How many of the person's assigned issues one page of a read asks for.
const ASSIGNED_PAGE_SIZE: u32 = 100;

/// How many pages of the person's assigned issues one read takes, so 1,000 at most, the bound the
/// other providers keep. Past it, the least recently updated wait until they change.
const ASSIGNED_PAGE_LIMIT: u32 = 10;

/// The resolved states a close prefers, in order, when a project has several. YouTrack's default
/// workflow resolves to `Fixed`, and its Kanban and Scrum templates to `Done`; the rest are the
/// conventional names of finished work. A state such as `Duplicate` or `Won't fix` is resolved
/// too, but is never what finishing the work means.
const PREFERRED_DONE_STATES: [&str; 6] = [
    "Done",
    "Fixed",
    "Resolved",
    "Completed",
    "Verified",
    "Closed",
];

/// Links through YouTrack token accounts.
pub struct YoutrackProvider {
    youtrack: Youtrack,
    database: Pool,
    cipher: Arc<Cipher>,
}

/// An account opened for calls: its site, its token, and the login the token belongs to.
struct OpenAccount {
    url: String,
    token: SecretString,
    login: String,
}

impl OpenAccount {
    const fn site(&self) -> Site<'_> {
        Site {
            url: self.url.as_str(),
            token: &self.token,
        }
    }
}

impl YoutrackProvider {
    pub const fn new(youtrack: Youtrack, database: Pool, cipher: Arc<Cipher>) -> Self {
        Self {
            youtrack,
            database,
            cipher,
        }
    }

    /// Opens a YouTrack token account, returning the database connection before any call.
    async fn open(&self, account_id: Uuid) -> Result<OpenAccount, LinkError> {
        let mut connection = self
            .database
            .get()
            .await
            .context("no database connection available")
            .map_err(LinkError::Internal)?;
        let account = token_account::find(&mut connection, account_id)
            .await
            .map_err(|error| match error {
                diesel::result::Error::NotFound => {
                    LinkError::NotFound("no YouTrack account has that id".to_owned())
                }
                other => other.into(),
            })?;
        drop(connection);
        if account.kind != AccountKind::Youtrack.as_str() {
            return Err(LinkError::Invalid(
                "that account's key is not a YouTrack token".to_owned(),
            ));
        }
        let url = account
            .site_url
            .clone()
            .context("a YouTrack account names its site, and this one has none")
            .map_err(LinkError::Internal)?;
        let token = account.credential(&self.cipher).map_err(|_open_error| {
            LinkError::Internal(anyhow::anyhow!("the stored token cannot be decrypted"))
        })?;
        Ok(OpenAccount {
            url,
            token,
            login: account.identity,
        })
    }

    async fn find(
        &self,
        account_id: Uuid,
        kind: LinkKind,
        reference: &str,
    ) -> Result<Remote, LinkError> {
        if kind != LinkKind::Issue {
            return Err(LinkError::Invalid(
                "YouTrack links issues only; a pull request is linked through GitHub".to_owned(),
            ));
        }
        let reference = reference.trim();
        if !is_issue_key(reference) {
            return Err(LinkError::Invalid(
                "a YouTrack issue is named by its key, such as JL-12".to_owned(),
            ));
        }
        let opened = self.open(account_id).await?;
        let issue = self.youtrack.issue(&opened.site(), reference).await?;
        Ok(remote(&opened, issue))
    }

    async fn read(&self, link: &ActionItemLink) -> Result<Remote, LinkError> {
        let opened = self.open(link.credential().id).await?;
        // The database id, which survives the issue moving to another project.
        let issue = self
            .youtrack
            .issue(&opened.site(), &link.external_id)
            .await?;
        Ok(remote(&opened, issue))
    }

    /// Searches for the linked issues updated since `since`, a few dozen keys at a time. A search
    /// YouTrack refuses is read one issue at a time instead, so one issue it cannot find never
    /// hides the changes of the rest.
    async fn changes(
        &self,
        account_id: Uuid,
        links: &[ActionItemLink],
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<Remote>, LinkError> {
        let opened = self.open(account_id).await?;
        let updated_since = since.map(|since| since - TimeDelta::minutes(CURSOR_OVERLAP_MINUTES));
        let is_news = |issue: &Issue| updated_since.is_none_or(|since| issue.updated >= since);

        let mut remotes = Vec::new();
        for chunk in links.chunks(KEYS_PER_SEARCH) {
            // Keys are what YouTrack answered when the link was made; anything else is left out
            // rather than written into a query.
            let keys: Vec<&str> = chunk
                .iter()
                .map(|link| link.external_key.as_str())
                .filter(|key| is_issue_key(key))
                .collect();
            if keys.is_empty() {
                continue;
            }
            let query = format!("issue id: {}", keys.join(", "));
            let top = u32::try_from(keys.len()).unwrap_or(u32::MAX);
            match self.youtrack.search(&opened.site(), &query, 0, top).await {
                Ok(found) => remotes.extend(
                    found
                        .into_iter()
                        .filter(is_news)
                        .map(|issue| remote(&opened, issue)),
                ),
                Err(YoutrackError::Refused(message)) => {
                    event!(
                        name: "links.youtrack.search.refused",
                        Level::DEBUG,
                        error.message = %message,
                        "YouTrack refused a search of linked issues; reading them one at a time",
                    );
                    for link in chunk {
                        match self.youtrack.issue(&opened.site(), &link.external_id).await {
                            Ok(issue) if is_news(&issue) => remotes.push(remote(&opened, issue)),
                            Ok(_unchanged) => {}
                            Err(error) => event!(
                                name: "links.youtrack.issue.unreadable",
                                Level::INFO,
                                link.id = %link.id,
                                error.message = %error,
                                "a linked YouTrack issue cannot be read; it is left as it was",
                            ),
                        }
                    }
                }
                Err(other) => return Err(other.into()),
            }
        }
        Ok(remotes)
    }

    /// The unresolved issues assigned to the token's user in the projects `content` chose, the
    /// way the connector's reader bounds what it reads.
    async fn assigned(
        &self,
        account_id: Uuid,
        content: &Value,
    ) -> Result<AssignedIssues, LinkError> {
        let content: Content = serde_json::from_value(content.clone()).map_err(|error| {
            LinkError::Internal(anyhow::anyhow!(
                "a YouTrack connector's content is not a selection of projects: {error}"
            ))
        })?;
        let opened = self.open(account_id).await?;
        let reachable = self.youtrack.list_projects(&opened.site()).await?;
        let Some(projects) = project_query(&content.projects, &reachable.items) else {
            // Every chosen project is gone or archived, which the connector's checks say;
            // nothing is assigned in a project nobody can read.
            event!(
                name: "links.youtrack.assigned.no_projects",
                Level::DEBUG,
                account.id = %account_id,
                "a YouTrack connector reads no project the token reaches",
            );
            return Ok(AssignedIssues::default());
        };
        // `for: me` is the token's user as assignee, and `#Unresolved` every state YouTrack does
        // not count as resolved.
        let query = format!("{projects} for: me #Unresolved");

        let mut items = Vec::new();
        for page in 0..ASSIGNED_PAGE_LIMIT {
            let found = self
                .youtrack
                .search(
                    &opened.site(),
                    query.trim(),
                    page * ASSIGNED_PAGE_SIZE,
                    ASSIGNED_PAGE_SIZE,
                )
                .await?;
            let is_full = found.len() == ASSIGNED_PAGE_SIZE as usize;
            items.extend(found.into_iter().map(|issue| remote(&opened, issue)));
            if !is_full {
                return Ok(AssignedIssues {
                    items,
                    truncated: false,
                });
            }
        }
        // Every page came back full, so more may be assigned than one read takes.
        Ok(AssignedIssues {
            items,
            truncated: true,
        })
    }

    async fn close(&self, link: &ActionItemLink) -> Result<WriteOutcome, LinkError> {
        let opened = self.open(link.credential().id).await?;
        let issue = self
            .youtrack
            .issue(&opened.site(), &link.external_id)
            .await?;
        if issue.resolved {
            return Ok(WriteOutcome::AlreadyDone);
        }
        let Some(field) = self
            .youtrack
            .state_field(&opened.site(), &issue.project_id)
            .await?
        else {
            return Ok(WriteOutcome::Waiting(format!(
                "project {} has no state field to move {} to a resolved state",
                issue.project_short_name, issue.key
            )));
        };
        let value = match done_state(&field) {
            Ok(value) => value,
            Err(waiting) => {
                return Ok(WriteOutcome::Waiting(format!(
                    "{waiting}, so {} waits",
                    issue.key
                )));
            }
        };
        self.youtrack
            .set_state(&opened.site(), &issue.id, &field.name, &value)
            .await?;
        Ok(WriteOutcome::Landed)
    }

    async fn comment(&self, link: &ActionItemLink, body: &str) -> Result<(), LinkError> {
        let opened = self.open(link.credential().id).await?;
        self.youtrack
            .add_comment(&opened.site(), &link.external_id, body)
            .await?;
        Ok(())
    }

    /// The projects the account reaches, archived ones left out.
    async fn destinations(&self, account_id: Uuid) -> Result<Vec<Destination>, LinkError> {
        let opened = self.open(account_id).await?;
        let projects = self.youtrack.list_projects(&opened.site()).await?;
        let mut destinations: Vec<Destination> = projects
            .items
            .into_iter()
            .map(|project| Destination {
                reference: project.short_name,
                name: project.name,
            })
            .collect();
        destinations.sort_by(|first, second| first.name.cmp(&second.name));
        Ok(destinations)
    }

    async fn destination(
        &self,
        account_id: Uuid,
        reference: &str,
    ) -> Result<Destination, LinkError> {
        let opened = self.open(account_id).await?;
        let project = self.project(&opened, reference).await?;
        Ok(Destination {
            reference: project.short_name,
            name: project.name,
        })
    }

    async fn file(&self, issue: IssueToFile<'_>) -> Result<Remote, LinkError> {
        let opened = self.open(issue.credential_id).await?;
        let project = self.project(&opened, issue.destination).await?;
        if let Some(since) = issue.tried_since {
            let since = since - TimeDelta::minutes(CURSOR_OVERLAP_MINUTES);
            let query = format!("project: {}", project.short_name);
            let recent = self
                .youtrack
                .search(&opened.site(), &query, 0, FILED_SEARCH_SIZE)
                .await?;
            let earlier = recent.into_iter().find(|found| {
                found.created >= since
                    && found.summary.trim() == issue.title.trim()
                    && found
                        .reporter
                        .as_ref()
                        .is_some_and(|reporter| reporter.login == opened.login)
            });
            if let Some(earlier) = earlier {
                return Ok(remote(&opened, earlier));
            }
        }

        let created = self
            .youtrack
            .create_issue(&opened.site(), &project.id, issue.title, issue.body)
            .await?;
        Ok(remote(&opened, created))
    }

    /// The project `reference` names by its short name, among those the account reaches.
    async fn project(&self, opened: &OpenAccount, reference: &str) -> Result<Project, LinkError> {
        let reference = reference.trim();
        let projects = self.youtrack.list_projects(&opened.site()).await?;
        projects
            .items
            .into_iter()
            .find(|project| project.short_name.eq_ignore_ascii_case(reference))
            .ok_or_else(|| {
                LinkError::NotFound(format!(
                    "the YouTrack account reaches no project {reference}"
                ))
            })
    }
}

/// The resolved value of a project's state field that finishing an issue moves it to, or why
/// there is none to choose.
///
/// A field with exactly one resolved value uses it. One with several uses the first of
/// [`PREFERRED_DONE_STATES`] it has, so a default project resolves to `Fixed` and never to
/// `Duplicate`. One with several and none of those names waits, saying which it has, rather than
/// guess between them.
///
/// # Errors
/// Why the field offers no state to move to.
pub fn done_state(field: &StateField) -> Result<String, String> {
    let resolved: Vec<&str> = field
        .values
        .iter()
        .filter(|value| value.is_resolved)
        .map(|value| value.name.as_str())
        .collect();
    if let [only] = resolved.as_slice() {
        return Ok((*only).to_owned());
    }
    let preferred = PREFERRED_DONE_STATES.iter().find_map(|wanted| {
        resolved
            .iter()
            .find(|name| name.eq_ignore_ascii_case(wanted))
    });
    if let Some(name) = preferred {
        return Ok((*name).to_owned());
    }
    if resolved.is_empty() {
        return Err(format!("{} has no resolved state", field.name));
    }
    Err(format!(
        "{} has several resolved states ({}) and none is a plain done state",
        field.name,
        resolved.join(", ")
    ))
}

/// An issue as a link reads it. The assignee is the user when it is the token's own user.
fn remote(opened: &OpenAccount, issue: Issue) -> Remote {
    let owner = match &issue.assignee {
        Some(assignee) if assignee.login == opened.login => Owner::User,
        Some(assignee) => Owner::Other {
            name: assignee.full_name.clone(),
        },
        None => Owner::Nobody,
    };
    let state = if issue.resolved {
        LinkState::Done
    } else {
        LinkState::Open
    };
    Remote {
        external_id: issue.id,
        url: issue_url(&opened.url, &issue.key),
        key: issue.key,
        title: issue.summary,
        state,
        status: issue.state.unwrap_or_default(),
        owner,
        assignee: issue.assignee.map(|assignee| assignee.full_name),
        priority: issue.priority,
        due_date: None,
    }
}

impl From<YoutrackError> for LinkError {
    fn from(error: YoutrackError) -> Self {
        match error {
            YoutrackError::Unauthorized(message) => Self::Upstream(message.to_owned()),
            YoutrackError::NotFound => Self::NotFound(error.to_string()),
            YoutrackError::Refused(message) => Self::Upstream(message),
        }
    }
}

impl Provider for YoutrackProvider {
    fn find<'call>(
        &'call self,
        credential_id: Uuid,
        kind: LinkKind,
        reference: &'call str,
    ) -> BoxFuture<'call, Result<Remote, LinkError>> {
        Self::find(self, credential_id, kind, reference).boxed()
    }

    fn read<'call>(
        &'call self,
        link: &'call ActionItemLink,
    ) -> BoxFuture<'call, Result<Remote, LinkError>> {
        Self::read(self, link).boxed()
    }

    fn changes<'call>(
        &'call self,
        credential_id: Uuid,
        links: &'call [ActionItemLink],
        since: Option<DateTime<Utc>>,
    ) -> BoxFuture<'call, Result<Vec<Remote>, LinkError>> {
        Self::changes(self, credential_id, links, since).boxed()
    }

    fn find_container<'call>(
        &'call self,
        _credential_id: Uuid,
        _kind: ContainerKind,
        _reference: &'call str,
    ) -> BoxFuture<'call, Result<RemoteContainer, LinkError>> {
        async { Err(no_containers()) }.boxed()
    }

    fn children<'call>(
        &'call self,
        _container: &'call InitiativeLink,
    ) -> BoxFuture<'call, Result<Children, LinkError>> {
        async { Err(no_containers()) }.boxed()
    }

    fn assigned<'call>(
        &'call self,
        credential_id: Uuid,
        content: &'call Value,
    ) -> BoxFuture<'call, Result<AssignedIssues, LinkError>> {
        Self::assigned(self, credential_id, content).boxed()
    }

    fn close<'call>(
        &'call self,
        link: &'call ActionItemLink,
    ) -> BoxFuture<'call, Result<WriteOutcome, LinkError>> {
        Self::close(self, link).boxed()
    }

    fn comment<'call>(
        &'call self,
        link: &'call ActionItemLink,
        body: &'call str,
    ) -> BoxFuture<'call, Result<(), LinkError>> {
        Self::comment(self, link, body).boxed()
    }
    fn destinations(
        &self,
        credential_id: Uuid,
    ) -> BoxFuture<'_, Result<Vec<Destination>, LinkError>> {
        Self::destinations(self, credential_id).boxed()
    }

    fn destination<'call>(
        &'call self,
        credential_id: Uuid,
        reference: &'call str,
    ) -> BoxFuture<'call, Result<Destination, LinkError>> {
        Self::destination(self, credential_id, reference).boxed()
    }

    fn file<'call>(
        &'call self,
        issue: IssueToFile<'call>,
    ) -> BoxFuture<'call, Result<Remote, LinkError>> {
        Self::file(self, issue).boxed()
    }
}

fn no_containers() -> LinkError {
    LinkError::Invalid(
        "initiatives follow Jira epics and saved filters and GitHub milestones and labels; \
         YouTrack issues are linked one at a time"
            .to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::youtrack::StateValue;

    fn field(values: &[(&str, bool)]) -> StateField {
        StateField {
            name: "State".to_owned(),
            values: values
                .iter()
                .map(|(name, is_resolved)| StateValue {
                    name: (*name).to_owned(),
                    is_resolved: *is_resolved,
                })
                .collect(),
        }
    }

    #[test]
    fn a_default_project_resolves_to_fixed_never_to_duplicate() {
        let default = field(&[
            ("Submitted", false),
            ("Open", false),
            ("In Progress", false),
            ("Duplicate", true),
            ("Fixed", true),
            ("Won't fix", true),
            ("Verified", true),
        ]);
        assert_eq!(done_state(&default), Ok("Fixed".to_owned()));
    }

    #[test]
    fn one_resolved_state_is_used_whatever_it_is_called() {
        let custom = field(&[("Backlog", false), ("Shipped", true)]);
        assert_eq!(done_state(&custom), Ok("Shipped".to_owned()));
    }

    #[test]
    fn several_unfamiliar_resolved_states_wait_naming_them() {
        let unclear = field(&[("Open", false), ("Shipped", true), ("Dropped", true)]);
        let waiting = done_state(&unclear).unwrap_err();
        assert!(waiting.contains("Shipped, Dropped"), "{waiting}");

        let none = field(&[("Open", false)]);
        assert_eq!(
            done_state(&none),
            Err("State has no resolved state".to_owned())
        );
    }
}
