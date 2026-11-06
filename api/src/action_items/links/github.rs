// Copyright © 2026 Jalapeno Labs

//! Links through a GitHub personal access token: issues and pull requests on items, milestones
//! or labels on one repository as initiatives' containers, and the open issues assigned to the
//! token's account in the connector's chosen repositories, which come in as items.
//!
//! GitHub reports an issue's state and, once closed, why: `completed` or `not_planned`. A
//! pull request is an issue that carries whether it merged. [`remote`] turns those into one
//! [`LinkState`], which is all the rules above this read.
//!
//! Every call goes to the fixed `api.github.com` with the credential's token, so a link
//! reaches no further than the token does.

use std::collections::BTreeMap;
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
use crate::connectors::kinds::github::Content;
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::github::issues::{Issue, IssueQuery, IssueRef, PullRequest};
use crate::github::{Github, Repository};
use crate::models::action_item::Owner;
use crate::models::action_item_link::{ActionItemLink, LinkKind, LinkState};
use crate::models::github_credential;
use crate::models::initiative_link::{ContainerKind, InitiativeLink};
use crate::routes::v1::jira_credentials::Selection;

/// How many pages of a repository's recent changes one pass reads, so 300 issues and pull
/// requests. A repository busier than that since the cursor is read link by link instead.
const CHANGE_PAGE_LIMIT: usize = 3;

/// How many pages of a container's children the watcher reads, so 1,000 children at most.
/// A container past it is marked truncated and its children are never taken out.
const CHILD_PAGE_LIMIT: usize = 10;

/// How many pages of assigned issues one listing takes, so 1,000 at most, the same bound as a
/// container's children: the account's listing, or each chosen repository's. Past it, the least
/// recently updated wait until they change.
const ASSIGNED_PAGE_LIMIT: usize = 10;

/// Minutes taken off every "updated since" listing, so an issue updated in the moment the
/// cursor was taken, or while GitHub's clock and Elysium's disagree a little, is read again
/// rather than missed. Applying a change twice changes nothing.
const CURSOR_OVERLAP_MINUTES: i64 = 5;

/// A token, opened, with the account it acts as.
struct OpenToken {
    token: SecretString,
    login: String,
}

/// Links through GitHub tokens.
pub struct GithubProvider {
    github: Github,
    database: Pool,
    cipher: Arc<Cipher>,
}

impl GithubProvider {
    pub const fn new(github: Github, database: Pool, cipher: Arc<Cipher>) -> Self {
        Self {
            github,
            database,
            cipher,
        }
    }

    /// Opens a token, returning the database connection before any call to GitHub.
    async fn open(&self, credential_id: Uuid) -> Result<OpenToken, LinkError> {
        let mut connection = self
            .database
            .get()
            .await
            .context("no database connection available")
            .map_err(LinkError::Internal)?;
        let credential = github_credential::find(&mut connection, credential_id)
            .await
            .map_err(|error| match error {
                diesel::result::Error::NotFound => {
                    LinkError::NotFound("no GitHub credential has that id".to_owned())
                }
                other => other.into(),
            })?;
        drop(connection);
        let token = credential
            .token(&self.cipher)
            .context("the stored token cannot be decrypted")
            .map_err(LinkError::Internal)?;
        Ok(OpenToken {
            token,
            login: credential.login,
        })
    }

    async fn find(
        &self,
        credential_id: Uuid,
        kind: LinkKind,
        reference: &str,
    ) -> Result<Remote, LinkError> {
        let issue_ref = parse_issue(reference)?;
        let opened = self.open(credential_id).await?;
        let issue = self.issue(&opened, &issue_ref).await?;
        let is_pull_request = issue.pull_request.is_some();
        match (kind, is_pull_request) {
            (LinkKind::Issue, true) => Err(LinkError::Invalid(format!(
                "{issue_ref} is a pull request, not an issue"
            ))),
            (LinkKind::PullRequest, false) => Err(LinkError::Invalid(format!(
                "{issue_ref} is an issue, not a pull request"
            ))),
            _matching => Ok(remote(&opened, issue)),
        }
    }

    async fn read(&self, link: &ActionItemLink) -> Result<Remote, LinkError> {
        let issue_ref = parse_issue(&link.external_id)?;
        let opened = self.open(link.credential().id).await?;
        let issue = self.issue(&opened, &issue_ref).await?;
        Ok(remote(&opened, issue))
    }

    /// Lists each linked repository's issues updated since `since`, and keeps the linked
    /// ones. A repository with no cursor yet, or busier than [`CHANGE_PAGE_LIMIT`] pages,
    /// is read link by link.
    async fn changes(
        &self,
        credential_id: Uuid,
        links: &[ActionItemLink],
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<Remote>, LinkError> {
        let opened = self.open(credential_id).await?;

        // Links by repository, keyed on the owner and name GitHub spelled when they were made.
        let mut by_repository: BTreeMap<String, Vec<IssueRef>> = BTreeMap::new();
        for link in links {
            let Ok(issue_ref) = parse_issue(&link.external_id) else {
                event!(
                    name: "links.github.link.unreadable",
                    Level::WARN,
                    link.id = %link.id,
                    "a GitHub link names no issue; it is left as it was",
                );
                continue;
            };
            let repository = format!(
                "{}/{}",
                issue_ref.repository.owner, issue_ref.repository.name
            );
            by_repository.entry(repository).or_default().push(issue_ref);
        }

        let mut remotes = Vec::new();
        for issue_refs in by_repository.into_values() {
            let repository = &issue_refs[0].repository;
            let listed = match since {
                Some(since) => {
                    let query = IssueQuery {
                        since: Some(since - chrono::TimeDelta::minutes(CURSOR_OVERLAP_MINUTES)),
                        ..IssueQuery::default()
                    };
                    let listing = self
                        .github
                        .list_issues(&opened.token, repository, &query, CHANGE_PAGE_LIMIT)
                        .await?;
                    (!listing.truncated).then_some(listing.items)
                }
                None => None,
            };

            let Some(listed) = listed else {
                for issue_ref in &issue_refs {
                    let Some(issue) = self.github.issue(&opened.token, issue_ref).await? else {
                        event!(
                            name: "links.github.issue.unreadable",
                            Level::INFO,
                            github.issue = %issue_ref,
                            "a linked GitHub issue cannot be read; it is left as it was",
                        );
                        continue;
                    };
                    remotes.extend(self.settled_remote(&opened, issue).await);
                }
                continue;
            };
            for issue in listed {
                let is_linked = issue_refs.iter().any(|issue_ref| {
                    issue_ref.number == issue.number
                        && issue.repository.eq_ignore_ascii_case(&format!(
                            "{}/{}",
                            issue_ref.repository.owner, issue_ref.repository.name
                        ))
                });
                if is_linked {
                    remotes.extend(self.settled_remote(&opened, issue).await);
                }
            }
        }
        Ok(remotes)
    }

    /// A linked issue read by the watcher, with a closed pull request's merge settled, or
    /// `None` when GitHub will not say whether it merged, such as for a token without Pull
    /// requests read. That link is left as it was, so one pull request never stops the rest
    /// of the credential's links and containers.
    async fn settled_remote(&self, opened: &OpenToken, issue: Issue) -> Option<Remote> {
        let reference = issue.reference();
        match self.settle_merge(opened, issue).await {
            Ok(issue) => Some(remote(opened, issue)),
            Err(error) => {
                event!(
                    name: "links.github.merge.unreadable",
                    Level::INFO,
                    github.issue = %reference,
                    error.message = %error,
                    "whether a linked pull request merged cannot be read; it is left as it was",
                );
                None
            }
        }
    }

    async fn find_container(
        &self,
        credential_id: Uuid,
        kind: ContainerKind,
        reference: &str,
    ) -> Result<RemoteContainer, LinkError> {
        let opened = self.open(credential_id).await?;
        self.find_container_with(&opened, kind, reference).await
    }

    /// The container `reference` names, read with a token already opened.
    async fn find_container_with(
        &self,
        opened: &OpenToken,
        kind: ContainerKind,
        reference: &str,
    ) -> Result<RemoteContainer, LinkError> {
        match kind {
            ContainerKind::Milestone => {
                let milestone_ref = parse_issue(reference)?;
                let repository = &milestone_ref.repository;
                let milestone = self
                    .github
                    .milestone(&opened.token, repository, milestone_ref.number)
                    .await?
                    .ok_or_else(|| {
                        LinkError::NotFound(format!(
                            "{milestone_ref} is not a milestone the token can see"
                        ))
                    })?;
                let key = milestone_ref.to_string();
                Ok(RemoteContainer {
                    external_id: key.clone(),
                    key,
                    url: milestone.url,
                    title: milestone.title,
                })
            }
            ContainerKind::Label => {
                let (repository, name) = parse_label(reference)?;
                let label = self
                    .github
                    .label(&opened.token, &repository, &name)
                    .await?
                    .ok_or_else(|| {
                        LinkError::NotFound(format!(
                            "{}/{} has no label {name}",
                            repository.owner, repository.name
                        ))
                    })?;
                let key = format!("{}/{}:{}", repository.owner, repository.name, label.name);
                Ok(RemoteContainer {
                    external_id: key.clone(),
                    key,
                    url: label.url,
                    title: label.name,
                })
            }
            ContainerKind::Epic | ContainerKind::Filter => Err(LinkError::Invalid(
                "GitHub contains issues by milestone or label".to_owned(),
            )),
        }
    }

    /// A milestone's or a label's issues, open and closed. Pull requests in them are left
    /// out: the issues they fix are the work.
    ///
    /// The container itself is read first, so one that is gone, such as a label renamed on
    /// GitHub, answers [`LinkError::NotFound`] and the watcher keeps its children. Listing
    /// by a label GitHub no longer has answers an empty page, which would take every child
    /// out.
    async fn children(&self, container: &InitiativeLink) -> Result<Children, LinkError> {
        let opened = self.open(container.credential().id).await?;
        let found = self
            .find_container_with(&opened, container.kind, &container.external_id)
            .await?;
        let (repository, issue_query) = match container.kind {
            ContainerKind::Milestone => {
                let milestone_ref = parse_issue(&container.external_id)?;
                let query = IssueQuery {
                    milestone: Some(milestone_ref.number),
                    ..IssueQuery::default()
                };
                (milestone_ref.repository, query)
            }
            ContainerKind::Label => {
                let (repository, _name) = parse_label(&container.external_id)?;
                let query = IssueQuery {
                    label: Some(found.title.as_str()),
                    ..IssueQuery::default()
                };
                (repository, query)
            }
            ContainerKind::Epic | ContainerKind::Filter => {
                return Err(LinkError::Invalid(
                    "GitHub contains issues by milestone or label".to_owned(),
                ));
            }
        };
        let listing = self
            .github
            .list_issues(&opened.token, &repository, &issue_query, CHILD_PAGE_LIMIT)
            .await?;
        let title = found.title.clone();
        let items = listing
            .items
            .into_iter()
            .filter(|issue| issue.pull_request.is_none())
            .map(|issue| remote(&opened, issue))
            .collect();
        Ok(Children {
            title,
            items,
            truncated: listing.truncated,
        })
    }

    /// The open issues assigned to the token's account in the repositories `content` chose,
    /// most recently updated first, bounded in what is asked of GitHub as Jira's and YouTrack's
    /// reads are bounded in their queries.
    ///
    /// A connector reading every repository takes one listing of everything assigned to the
    /// account, which costs the same however many repositories it spans. One that chose
    /// repositories lists each of them by assignee instead, a request per repository, so what
    /// is assigned elsewhere never spends the read's cap before a chosen repository is reached.
    ///
    /// # Errors
    /// A chosen repository the token cannot read fails the read, naming it, as a Jira or
    /// YouTrack query naming a project it cannot read does, so the connector shows which pick
    /// to change rather than reading less than it says.
    async fn assigned(
        &self,
        credential_id: Uuid,
        content: &Value,
    ) -> Result<AssignedIssues, LinkError> {
        let content: Content = serde_json::from_value(content.clone()).map_err(|error| {
            LinkError::Internal(anyhow::anyhow!(
                "a GitHub connector's content is not a selection of repositories: {error}"
            ))
        })?;
        let opened = self.open(credential_id).await?;
        let (listed, truncated) = match &content.repositories {
            Selection::All => {
                let listing = self
                    .github
                    .list_assigned_issues(&opened.token, ASSIGNED_PAGE_LIMIT)
                    .await?;
                (listing.items, listing.truncated)
            }
            Selection::Only(names) => {
                let query = IssueQuery {
                    open_only: true,
                    assignee: Some(opened.login.as_str()),
                    ..IssueQuery::default()
                };
                let mut listed = Vec::new();
                let mut truncated = false;
                for name in names {
                    let repository = parse_repository(name)?;
                    let listing = self
                        .github
                        .list_issues(&opened.token, &repository, &query, ASSIGNED_PAGE_LIMIT)
                        .await
                        .map_err(|error| {
                            LinkError::Upstream(format!("cannot read {name}'s issues: {error}"))
                        })?;
                    listed.extend(listing.items);
                    truncated |= listing.truncated;
                }
                // Each repository lists its own newest first; the read answers newest first
                // across all of them.
                listed.sort_by_key(|issue| std::cmp::Reverse(issue.updated_at));
                (listed, truncated)
            }
        };
        let items = listed
            .into_iter()
            .filter(|issue| issue.pull_request.is_none())
            .map(|issue| remote(&opened, issue))
            .collect();
        Ok(AssignedIssues { items, truncated })
    }

    async fn close(&self, link: &ActionItemLink) -> Result<WriteOutcome, LinkError> {
        if link.kind != LinkKind::Issue {
            return Err(LinkError::Invalid(
                "Elysium never closes or merges a pull request".to_owned(),
            ));
        }
        let issue_ref = parse_issue(&link.external_id)?;
        let opened = self.open(link.credential().id).await?;
        let issue = self.issue(&opened, &issue_ref).await?;
        if !issue.is_open {
            return Ok(WriteOutcome::AlreadyDone);
        }
        self.github.close_issue(&opened.token, &issue_ref).await?;
        Ok(WriteOutcome::Landed)
    }

    async fn comment(&self, link: &ActionItemLink, body: &str) -> Result<(), LinkError> {
        let issue_ref = parse_issue(&link.external_id)?;
        let opened = self.open(link.credential().id).await?;
        self.github.comment(&opened.token, &issue_ref, body).await?;
        Ok(())
    }

    /// The repositories the token lists that are not archived, since an archived one takes no
    /// issues.
    async fn destinations(&self, credential_id: Uuid) -> Result<Vec<Destination>, LinkError> {
        let opened = self.open(credential_id).await?;
        let listing = self.github.list_repositories(&opened.token).await?;
        let mut destinations: Vec<Destination> = listing
            .repositories
            .into_iter()
            .filter(|repository| !repository.is_archived)
            .map(|repository| Destination {
                reference: repository.full_name.clone(),
                name: repository.full_name,
            })
            .collect();
        destinations.sort_by(|first, second| first.name.cmp(&second.name));
        Ok(destinations)
    }

    async fn destination(
        &self,
        credential_id: Uuid,
        reference: &str,
    ) -> Result<Destination, LinkError> {
        let repository = parse_repository(reference)?;
        let opened = self.open(credential_id).await?;
        let access = self
            .github
            .repository_access(&opened.token, &repository)
            .await?
            .ok_or_else(|| {
                LinkError::NotFound(format!(
                    "{reference} does not exist or the token cannot see it"
                ))
            })?;
        Ok(Destination {
            reference: access.full_name.clone(),
            name: access.full_name,
        })
    }

    async fn file(&self, issue: IssueToFile<'_>) -> Result<Remote, LinkError> {
        let repository = parse_repository(issue.destination)?;
        let opened = self.open(issue.credential_id).await?;
        if let Some(since) = issue.tried_since {
            let query = IssueQuery {
                since: Some(since - TimeDelta::minutes(CURSOR_OVERLAP_MINUTES)),
                creator: Some(&opened.login),
                ..IssueQuery::default()
            };
            let recent = self
                .github
                .list_issues(&opened.token, &repository, &query, 1)
                .await?;
            let earlier = recent.items.into_iter().find(|found| {
                found.pull_request.is_none() && found.title.trim() == issue.title.trim()
            });
            if let Some(earlier) = earlier {
                return Ok(remote(&opened, earlier));
            }
        }

        let created = self
            .github
            .create_issue(&opened.token, &repository, issue.title, issue.body)
            .await?;
        let issue_ref = IssueRef {
            repository,
            number: created.number,
        };
        let read = self.issue(&opened, &issue_ref).await?;
        Ok(remote(&opened, read))
    }

    async fn issue(&self, opened: &OpenToken, issue_ref: &IssueRef) -> Result<Issue, LinkError> {
        let issue = self
            .github
            .issue(&opened.token, issue_ref)
            .await?
            .ok_or_else(|| {
                LinkError::NotFound(format!(
                    "{issue_ref} does not exist or the token cannot see it"
                ))
            })?;
        self.settle_merge(opened, issue).await
    }

    /// Confirms whether a closed pull request merged, from the pull request itself, when
    /// the issue payload does not say it did. An open pull request, a merged one, and an
    /// issue pass through unread.
    async fn settle_merge(&self, opened: &OpenToken, mut issue: Issue) -> Result<Issue, LinkError> {
        let needs_asking = !issue.is_open
            && issue
                .pull_request
                .is_some_and(|pull_request| !pull_request.merged);
        if !needs_asking {
            return Ok(issue);
        }
        let pull_request_ref = parse_issue(&issue.reference())?;
        let merged = self
            .github
            .pull_request_merged(&opened.token, &pull_request_ref)
            .await?;
        issue.pull_request = Some(PullRequest { merged });
        Ok(issue)
    }
}

/// `owner/name#12`, checked before it reaches a URL.
fn parse_issue(reference: &str) -> Result<IssueRef, LinkError> {
    IssueRef::parse(reference).ok_or_else(|| {
        LinkError::Invalid(format!(
            "{reference} is not a GitHub reference; one reads owner/name#12"
        ))
    })
}

/// `owner/name`, checked before it reaches a URL.
fn parse_repository(reference: &str) -> Result<Repository, LinkError> {
    Repository::from_url(&format!("https://github.com/{}", reference.trim())).ok_or_else(|| {
        LinkError::Invalid(format!(
            "{reference} is not a GitHub repository; one reads owner/name"
        ))
    })
}

/// `owner/name:label`, checked before it reaches a URL.
fn parse_label(reference: &str) -> Result<(Repository, String), LinkError> {
    let invalid = || {
        LinkError::Invalid(format!(
            "{reference} is not a GitHub label; one reads owner/name:label"
        ))
    };
    let (repository, name) = reference.trim().split_once(':').ok_or_else(invalid)?;
    let repository =
        Repository::from_url(&format!("https://github.com/{repository}")).ok_or_else(invalid)?;
    let name = name.trim();
    if name.is_empty() || matches!(name, "." | "..") {
        return Err(invalid());
    }
    Ok((repository, name.to_owned()))
}

/// An issue or pull request as a link reads it. It is the user's when the token's own account
/// is any of its assignees, since GitHub lets several people share one; otherwise it is its first
/// assignee's.
fn remote(opened: &OpenToken, issue: Issue) -> Remote {
    let state = state_of(&issue);
    let status = match state {
        LinkState::Open => "open",
        LinkState::Merged => "merged",
        LinkState::Done | LinkState::NotPlanned | LinkState::ClosedUnmerged => "closed",
    };
    let is_mine = issue
        .assignees
        .iter()
        .chain(&issue.assignee)
        .any(|login| login.eq_ignore_ascii_case(&opened.login));
    let owner = if is_mine {
        Owner::User
    } else {
        issue
            .assignee
            .clone()
            .map_or(Owner::Nobody, |name| Owner::Other { name })
    };
    Remote {
        external_id: issue.reference(),
        key: issue.reference(),
        url: issue.url,
        title: issue.title,
        state,
        status: status.to_owned(),
        owner,
        assignee: issue.assignee,
        priority: None,
        due_date: None,
    }
}

/// GitHub's state and reason as one [`LinkState`].
pub fn state_of(issue: &Issue) -> LinkState {
    match (&issue.pull_request, issue.is_open) {
        (_any, true) => LinkState::Open,
        (Some(pull_request), false) if pull_request.merged => LinkState::Merged,
        (Some(_unmerged), false) => LinkState::ClosedUnmerged,
        (None, false) if issue.state_reason.as_deref() == Some("not_planned") => {
            LinkState::NotPlanned
        }
        (None, false) => LinkState::Done,
    }
}

impl Provider for GithubProvider {
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
        credential_id: Uuid,
        kind: ContainerKind,
        reference: &'call str,
    ) -> BoxFuture<'call, Result<RemoteContainer, LinkError>> {
        Self::find_container(self, credential_id, kind, reference).boxed()
    }

    fn children<'call>(
        &'call self,
        container: &'call InitiativeLink,
    ) -> BoxFuture<'call, Result<Children, LinkError>> {
        Self::children(self, container).boxed()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(is_open: bool, reason: Option<&str>, pull_request: Option<bool>) -> Issue {
        Issue {
            repository: "JalapenoLabs/Elysium".to_owned(),
            number: 12,
            title: "Watch links".to_owned(),
            url: "https://github.com/JalapenoLabs/Elysium/issues/12".to_owned(),
            is_open,
            state_reason: reason.map(str::to_owned),
            assignee: None,
            assignees: Vec::new(),
            pull_request: pull_request.map(|merged| PullRequest { merged }),
            updated_at: None,
        }
    }

    #[test]
    fn githubs_state_and_reason_become_one_link_state() {
        assert_eq!(state_of(&issue(true, None, None)), LinkState::Open);
        assert_eq!(
            state_of(&issue(true, Some("reopened"), None)),
            LinkState::Open
        );
        assert_eq!(
            state_of(&issue(false, Some("completed"), None)),
            LinkState::Done
        );
        assert_eq!(
            state_of(&issue(false, None, None)),
            LinkState::Done,
            "an issue closed before GitHub kept reasons counts as completed"
        );
        assert_eq!(
            state_of(&issue(false, Some("not_planned"), None)),
            LinkState::NotPlanned
        );
        assert_eq!(state_of(&issue(true, None, Some(false))), LinkState::Open);
        assert_eq!(state_of(&issue(false, None, Some(true))), LinkState::Merged);
        assert_eq!(
            state_of(&issue(false, None, Some(false))),
            LinkState::ClosedUnmerged
        );
    }

    #[test]
    fn labels_are_named_owner_name_and_label() {
        let (repository, name) = parse_label("JalapenoLabs/Elysium:good first issue").expect("ok");
        assert_eq!(repository.owner, "JalapenoLabs");
        assert_eq!(name, "good first issue");

        for refused in [
            "JalapenoLabs/Elysium",
            "JalapenoLabs/Elysium:",
            "../x:bug",
            "x:..",
        ] {
            parse_label(refused).expect_err(refused);
        }
    }
}
