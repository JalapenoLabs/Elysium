// Copyright © 2026 Jalapeno Labs

//! Links through a Jira Cloud credential: issues on items, and epics or saved filters as
//! initiatives' containers.
//!
//! Every call goes through the credential's allowlist, the same way the Jira routes do:
//! an issue by key through `allowed_issue`, which checks the key's project before the call
//! and the project Jira answers with after it, and every search through `bounded_jql`, so
//! Jira itself never looks outside the allowed projects. An issue that moved outside the
//! list is skipped by the watcher and refused to a write.
//!
//! Resolving an item moves its issue along the project's done transition: the transition
//! into the `done` status chosen for the project (`crate::models::jira_done_transition`),
//! or into its only `done` status when it has one. Until a project with several is given a
//! choice, the move waits, and says so.

use std::sync::Arc;

use anyhow::Context;
use chrono::{DateTime, Utc};
use futures_util::FutureExt as _;
use futures_util::future::BoxFuture;
use tracing::{Level, event};
use uuid::Uuid;

use super::{Children, LinkError, Provider, Remote, RemoteContainer, WriteOutcome};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::jira::{Issue, Jira, JiraError, Status};
use crate::models::action_item::Owner;
use crate::models::action_item_link::{ActionItemLink, LinkKind, LinkState};
use crate::models::initiative_link::{ContainerKind, InitiativeLink};
use crate::models::jira_done_transition;
use crate::routes::v1::jira_credentials::allowlist::{bounded_jql, ensure_allowed};
use crate::routes::v1::jira_credentials::{OpenCredential, allowed_issue, open_with};

/// Jira's status category for work that is finished.
const DONE_CATEGORY: &str = "done";

/// How many issue ids one watcher search names. Keeps the JQL well under Jira's length
/// limits; a credential with more links is read in several searches.
const IDS_PER_SEARCH: usize = 50;

/// How many issues one page of a search asks for. A hundred is the most Jira sends.
const SEARCH_PAGE_SIZE: u32 = 100;

/// How many pages of changes one search follows. A search names at most
/// [`IDS_PER_SEARCH`] issues, fewer than one page holds, so one page is always all of them.
const CHANGE_PAGE_LIMIT: usize = 1;

/// How many pages of a container's children the watcher reads, so 1,000 children at most.
/// A container past it is marked truncated and its children are never taken out, since a
/// child missing from a cut short read may still be there.
const CHILD_PAGE_LIMIT: usize = 10;

/// Minutes added to every "updated since" search, so an issue updated in the moment the
/// cursor was taken, or while Jira's clock and Elysium's disagree a little, is read again
/// rather than missed. Applying a change twice changes nothing.
const CURSOR_OVERLAP_MINUTES: i64 = 5;

/// Links through Jira Cloud credentials.
pub struct JiraProvider {
    jira: Jira,
    database: Pool,
    cipher: Arc<Cipher>,
}

impl JiraProvider {
    pub const fn new(jira: Jira, database: Pool, cipher: Arc<Cipher>) -> Self {
        Self {
            jira,
            database,
            cipher,
        }
    }

    /// Opens a credential, returning the database connection before any call to Jira.
    async fn open(&self, credential_id: Uuid) -> Result<OpenCredential, LinkError> {
        let mut connection = self
            .database
            .get()
            .await
            .context("no database connection available")
            .map_err(LinkError::Internal)?;
        Ok(open_with(&mut connection, &self.cipher, credential_id).await?)
    }

    async fn find(
        &self,
        credential_id: Uuid,
        kind: LinkKind,
        reference: &str,
    ) -> Result<Remote, LinkError> {
        if kind != LinkKind::Issue {
            return Err(LinkError::Invalid(
                "Jira links issues only; a pull request is linked through GitHub".to_owned(),
            ));
        }
        let stored = self.open(credential_id).await?;
        let issue = allowed_issue(&self.jira, &stored, reference.trim()).await?;
        Ok(remote(&stored, issue))
    }

    async fn read(&self, link: &ActionItemLink) -> Result<Remote, LinkError> {
        let stored = self.open(link.credential().id).await?;
        let issue = allowed_issue(&self.jira, &stored, &link.external_key).await?;
        Ok(remote(&stored, issue))
    }

    /// Searches for the linked issues updated since `since`, a few dozen ids at a time.
    ///
    /// A search that Jira refuses, which is what naming an issue deleted since it was linked
    /// looks like, is read one issue at a time instead, so one deleted issue never hides
    /// the changes of the rest.
    async fn changes(
        &self,
        credential_id: Uuid,
        links: &[ActionItemLink],
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<Remote>, LinkError> {
        let stored = self.open(credential_id).await?;
        let updated_since = since.map(|since| {
            let minutes = (Utc::now() - since).num_minutes().max(0) + CURSOR_OVERLAP_MINUTES;
            format!(" AND updated >= -{minutes}m")
        });

        let mut remotes = Vec::new();
        for chunk in links.chunks(IDS_PER_SEARCH) {
            // Ids are what Jira answered when the link was made, digits only; anything else
            // is left out rather than written into JQL.
            let ids: Vec<&str> = chunk
                .iter()
                .map(|link| link.external_id.as_str())
                .filter(|id| !id.is_empty() && id.chars().all(|digit| digit.is_ascii_digit()))
                .collect();
            if ids.is_empty() {
                continue;
            }
            let conditions = format!(
                "id IN ({}){}",
                ids.join(", "),
                updated_since.as_deref().unwrap_or_default()
            );
            match self.search(&stored, &conditions, CHANGE_PAGE_LIMIT).await {
                Ok(found) => remotes.extend(found.items),
                Err(LinkError::Invalid(message)) => {
                    event!(
                        name: "links.jira.search.refused",
                        Level::DEBUG,
                        error.message = %message,
                        "Jira refused a search of linked issues; reading them one at a time",
                    );
                    for link in chunk {
                        match allowed_issue(&self.jira, &stored, &link.external_key).await {
                            Ok(issue) => remotes.push(remote(&stored, issue)),
                            Err(error) => event!(
                                name: "links.jira.issue.unreadable",
                                Level::INFO,
                                link.id = %link.id,
                                error.message = %error,
                                "a linked Jira issue cannot be read; it is left as it was",
                            ),
                        }
                    }
                }
                Err(other) => return Err(other),
            }
        }
        Ok(remotes)
    }

    async fn find_container(
        &self,
        credential_id: Uuid,
        kind: ContainerKind,
        reference: &str,
    ) -> Result<RemoteContainer, LinkError> {
        let stored = self.open(credential_id).await?;
        let reference = reference.trim();
        match kind {
            ContainerKind::Epic => {
                let epic = allowed_issue(&self.jira, &stored, reference).await?;
                Ok(RemoteContainer {
                    external_id: epic.id,
                    key: epic.key,
                    url: epic.url,
                    title: epic.summary,
                })
            }
            ContainerKind::Filter => {
                if reference.is_empty() || !reference.chars().all(|digit| digit.is_ascii_digit()) {
                    return Err(LinkError::Invalid(
                        "a saved filter is named by its id, as Jira lists it".to_owned(),
                    ));
                }
                let filter = self.jira.filter(&stored.site(), reference).await?;
                Ok(RemoteContainer {
                    external_id: filter.id.clone(),
                    key: filter.id,
                    url: filter.url,
                    title: filter.name,
                })
            }
            ContainerKind::Milestone | ContainerKind::Label => Err(LinkError::Invalid(
                "Jira contains issues by epic or saved filter".to_owned(),
            )),
        }
    }

    /// An epic's children are the issues whose parent it is; a saved filter's are the
    /// issues its JQL finds. Either way the search is bounded to the allowlist.
    async fn children(&self, container: &InitiativeLink) -> Result<Children, LinkError> {
        let stored = self.open(container.credential().id).await?;
        let (title, conditions) = match container.kind {
            ContainerKind::Epic => {
                let epic = allowed_issue(&self.jira, &stored, &container.external_key).await?;
                (epic.summary, format!("parent = {}", epic.id))
            }
            ContainerKind::Filter => {
                let filter = self
                    .jira
                    .filter(&stored.site(), &container.external_id)
                    .await?;
                (filter.name, format!("filter = {}", filter.id))
            }
            ContainerKind::Milestone | ContainerKind::Label => {
                return Err(LinkError::Invalid(
                    "Jira contains issues by epic or saved filter".to_owned(),
                ));
            }
        };
        let mut children = self.search(&stored, &conditions, CHILD_PAGE_LIMIT).await?;
        children.title = title;
        Ok(children)
    }

    async fn close(&self, link: &ActionItemLink) -> Result<WriteOutcome, LinkError> {
        let stored = self.open(link.credential().id).await?;
        let issue = allowed_issue(&self.jira, &stored, &link.external_key).await?;
        if is_done(issue.status.as_ref()) {
            return Ok(WriteOutcome::AlreadyDone);
        }

        let done_status = match self.done_status(&stored, &issue.project_key).await? {
            Ok(status) => status,
            Err(waiting) => return Ok(WriteOutcome::Waiting(waiting)),
        };
        let transitions = self.jira.transitions(&stored.site(), &issue.key).await?;
        let Some(transition) = transitions.iter().find(|transition| {
            transition
                .to
                .as_ref()
                .is_some_and(|to| to.id == done_status.id)
        }) else {
            let current = issue
                .status
                .as_ref()
                .map_or("its current status", |status| status.name.as_str());
            return Ok(WriteOutcome::Waiting(format!(
                "no transition from {current} leads to {} for {} right now",
                done_status.name, issue.key
            )));
        };

        self.jira
            .apply_transition(&stored.site(), &issue.key, &transition.id, None)
            .await?;
        Ok(WriteOutcome::Landed)
    }

    async fn comment(&self, link: &ActionItemLink, body: &str) -> Result<(), LinkError> {
        let stored = self.open(link.credential().id).await?;
        let issue = allowed_issue(&self.jira, &stored, &link.external_key).await?;
        self.jira
            .add_comment(&stored.site(), &issue.key, body)
            .await?;
        Ok(())
    }

    /// The `done` status a project's done transition leads into, or why there is none yet.
    ///
    /// The user's choice wins. Without one, a project with exactly one `done` status uses
    /// it; a project with several waits for the user to choose.
    async fn done_status(
        &self,
        stored: &OpenCredential,
        project_key: &str,
    ) -> Result<Result<Status, String>, LinkError> {
        let mut connection = self
            .database
            .get()
            .await
            .context("no database connection available")
            .map_err(LinkError::Internal)?;
        let chosen =
            jira_done_transition::find(&mut connection, stored.credential.id, project_key).await?;
        drop(connection);
        if let Some(chosen) = chosen {
            return Ok(Ok(Status {
                id: chosen.status_id,
                name: chosen.status_name,
                category: DONE_CATEGORY.to_owned(),
            }));
        }

        let done = done_statuses(&self.jira, stored, project_key).await?;
        match done.as_slice() {
            [only] => Ok(Ok(only.clone())),
            [] => Ok(Err(format!(
                "project {project_key} has no status in Jira's done category to move the issue \
                 to"
            ))),
            several => {
                let names: Vec<&str> = several.iter().map(|status| status.name.as_str()).collect();
                Ok(Err(format!(
                    "project {project_key} has several done statuses ({}); choose its done \
                     transition under Settings, Jira",
                    names.join(", ")
                )))
            }
        }
    }

    /// Runs a search bounded to the credential's allowlist, following its pages up to
    /// `page_limit`, and keeps only issues in allowed projects.
    async fn search(
        &self,
        stored: &OpenCredential,
        conditions: &str,
        page_limit: usize,
    ) -> Result<Children, LinkError> {
        let Some(jql) = bounded_jql(conditions, &stored.allowed.projects)? else {
            return Ok(Children::default());
        };

        let mut items = Vec::new();
        let mut next_page_token: Option<String> = None;
        // Whether Jira has pages left when the loop stops; the cap is what makes it true.
        let mut truncated = false;
        for _page in 0..page_limit {
            let found = self
                .jira
                .search(
                    &stored.site(),
                    &jql,
                    SEARCH_PAGE_SIZE,
                    next_page_token.as_deref(),
                )
                .await?;
            for issue in found.issues {
                let allowed = ensure_allowed(
                    &stored.allowed.projects,
                    &stored.credential.name,
                    &issue.project_key,
                );
                if allowed.is_ok() {
                    items.push(remote(stored, issue));
                }
            }
            if found.is_last || found.next_page_token.is_none() {
                truncated = false;
                break;
            }
            next_page_token = found.next_page_token;
            truncated = true;
        }
        Ok(Children {
            title: String::new(),
            items,
            truncated,
        })
    }
}

/// The `done` statuses of a project, the candidates for its done transition, refused unless
/// the credential may touch the project.
///
/// # Errors
/// Returns [`LinkError::Forbidden`] for a project outside the allowlist, and whatever Jira
/// answered otherwise.
pub async fn done_statuses(
    jira: &Jira,
    stored: &OpenCredential,
    project_key: &str,
) -> Result<Vec<Status>, LinkError> {
    ensure_allowed(
        &stored.allowed.projects,
        &stored.credential.name,
        project_key,
    )?;
    let statuses = jira
        .project_statuses(&stored.site(), project_key)
        .await
        .map_err(|error| match error {
            JiraError::NotFound => LinkError::NotFound(format!(
                "Jira has no project {project_key}, or the credential cannot see it"
            )),
            other => other.into(),
        })?;
    Ok(statuses
        .into_iter()
        .filter(|status| status.category == DONE_CATEGORY)
        .collect())
}

fn is_done(status: Option<&Status>) -> bool {
    status.is_some_and(|status| status.category == DONE_CATEGORY)
}

/// An issue as a link reads it. The assignee is the user when it is the credential's own
/// account.
fn remote(stored: &OpenCredential, issue: Issue) -> Remote {
    let owner = match &issue.assignee {
        Some(assignee) if assignee.account_id == stored.credential.account_id => Owner::User,
        Some(assignee) => Owner::Other {
            name: assignee.display_name.clone(),
        },
        None => Owner::Nobody,
    };
    let state = if is_done(issue.status.as_ref()) {
        LinkState::Done
    } else {
        LinkState::Open
    };
    Remote {
        external_id: issue.id,
        key: issue.key,
        url: issue.url,
        title: issue.summary,
        state,
        status: issue.status.map(|status| status.name).unwrap_or_default(),
        owner,
        assignee: issue.assignee.map(|assignee| assignee.display_name),
        priority: issue.priority,
        due_date: issue.due_date,
    }
}

impl Provider for JiraProvider {
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
}
