// Copyright © 2026 Jalapeno Labs

//! The external things action items and initiatives point at, reached through one
//! [`Provider`] trait with one implementation per provider.
//!
//! Routes, tools, and the watcher never match on a provider. They ask [`Links::provider`]
//! for a link's implementation and call it, the way `crate::storage` picks a storage
//! provider's client, so a new provider adds an implementation here and changes nothing
//! above it.
//!
//! A provider reads what a credential can reach and reports it as a [`Remote`] in Elysium's
//! terms: a [`LinkState`] rather than a Jira status category or GitHub's state and reason,
//! and an [`Owner`] rather than an account, where the credential's own account is the user.
//! Every call goes through the credential's own client, so Jira's allowlist bounds every
//! Jira call and GitHub's fixed host every GitHub call.
//!
//! What a provider's change does to an item is decided in [`rules`], without a database or
//! a provider, so each case in the design is a plain test.

pub mod github;
pub mod jira;
pub mod rules;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use futures_util::future::BoxFuture;
use serde::Serialize;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::crypto::Cipher;
use crate::database::Pool;
use crate::errors::ApiError;
use crate::github::{Github, GithubError};
use crate::jira::{Jira, JiraError};
use crate::models::action_item::{ActionItemPriority, Owner};
use crate::models::action_item_link::{
    ActionItemLink, LinkKind, LinkProvider, LinkState, Observation,
};
use crate::models::initiative_link::{ContainerKind, InitiativeLink};

use self::github::GithubProvider;
use self::jira::JiraProvider;

/// A linkable thing as its provider reports it now, in Elysium's terms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Remote {
    /// What the thing is, for good; see `ActionItemLink::external_id`.
    #[serde(skip)]
    pub external_id: String,
    /// What a person reads: `ELY-12`, or `owner/name#12`.
    pub key: String,
    pub url: String,
    pub title: String,
    pub state: LinkState,
    /// The provider's own word for its state: a Jira status, or `open`, `closed`, `merged`.
    pub status: String,
    /// Whose it is, in the item's terms.
    pub owner: Owner,
    /// The assignee as the provider names them, or `None` when nobody is assigned.
    pub assignee: Option<String>,
    /// The provider's own priority, such as Jira's `High`; GitHub has none.
    pub priority: Option<String>,
    /// The day it is due, when the provider keeps one; GitHub does not.
    pub due_date: Option<NaiveDate>,
}

impl Remote {
    /// What a link records about the thing.
    pub fn observation(&self) -> Observation {
        Observation {
            external_key: self.key.clone(),
            url: self.url.clone(),
            title: self.title.clone(),
            state: self.state,
            owner: self.owner.clone(),
        }
    }

    /// The priority an item created from the thing starts with. Never synced afterwards: the
    /// item's priority is the item's own.
    pub fn starting_priority(&self) -> ActionItemPriority {
        self.priority
            .as_deref()
            .and_then(|name| {
                PRIORITY_BY_PROVIDER_NAME
                    .iter()
                    .find(|(provider_name, _priority)| provider_name.eq_ignore_ascii_case(name))
            })
            .map_or(ActionItemPriority::Normal, |(_name, priority)| *priority)
    }

    /// The moment an item created from the thing is due: the last millisecond of its due
    /// day in UTC, since a provider keeps a day and the server has no viewer's zone.
    pub fn starting_due_at(&self) -> Option<DateTime<Utc>> {
        self.due_date
            .and_then(|date| date.and_hms_milli_opt(23, 59, 59, 999))
            .map(|moment| moment.and_utc())
    }
}

/// Jira's default priority names, and the ones common schemes add, as an item's priority.
/// A name not listed starts the item at `normal`.
const PRIORITY_BY_PROVIDER_NAME: [(&str, ActionItemPriority); 10] = [
    ("Highest", ActionItemPriority::Urgent),
    ("Blocker", ActionItemPriority::Urgent),
    ("Critical", ActionItemPriority::Urgent),
    ("High", ActionItemPriority::High),
    ("Major", ActionItemPriority::High),
    ("Medium", ActionItemPriority::Normal),
    ("Low", ActionItemPriority::Low),
    ("Minor", ActionItemPriority::Low),
    ("Lowest", ActionItemPriority::Low),
    ("Trivial", ActionItemPriority::Low),
];

/// A container as its provider reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteContainer {
    pub external_id: String,
    pub key: String,
    pub url: String,
    pub title: String,
}

/// A container's children, as many as the watcher reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Children {
    pub title: String,
    pub items: Vec<Remote>,
    /// The container held more than were read, so a child missing from `items` may still be
    /// in it.
    pub truncated: bool,
}

/// How a write went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOutcome {
    /// The provider took the write.
    Landed,
    /// The provider was already where the write would have put it, so nothing was sent.
    AlreadyDone,
    /// The write cannot be sent yet, for the reason given, such as a Jira project whose done
    /// transition is not chosen.
    Waiting(String),
}

/// Why a provider call could not be carried out. The message says what to fix.
#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    /// The request names something that cannot be linked as asked, such as an issue named
    /// as a pull request.
    #[error("{0}")]
    Invalid(String),
    /// The credential may not touch it, such as a Jira project outside its allowlist.
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    NotFound(String),
    /// The provider refused or could not be reached, in its own words.
    #[error("{0}")]
    Upstream(String),
    #[error(transparent)]
    Internal(anyhow::Error),
}

impl From<ApiError> for LinkError {
    fn from(error: ApiError) -> Self {
        match error {
            ApiError::BadRequest(message) => Self::Invalid(message),
            ApiError::Forbidden(message) => Self::Forbidden(message),
            ApiError::NotFound => Self::NotFound(
                "the provider has no such thing, or the credential cannot see it".to_owned(),
            ),
            ApiError::BadGateway(message) => Self::Upstream(message),
            ApiError::Internal(error) => Self::Internal(error),
            other => Self::Internal(anyhow::anyhow!("{other}")),
        }
    }
}

impl From<JiraError> for LinkError {
    fn from(error: JiraError) -> Self {
        ApiError::from(error).into()
    }
}

impl From<GithubError> for LinkError {
    fn from(error: GithubError) -> Self {
        ApiError::from(error).into()
    }
}

impl From<diesel::result::Error> for LinkError {
    fn from(error: diesel::result::Error) -> Self {
        Self::Internal(error.into())
    }
}

impl From<LinkError> for ApiError {
    fn from(error: LinkError) -> Self {
        match error {
            LinkError::Invalid(message) => Self::BadRequest(message),
            LinkError::Forbidden(message) => Self::Forbidden(message),
            LinkError::NotFound(_message) => Self::NotFound,
            LinkError::Upstream(message) => Self::BadGateway(message),
            LinkError::Internal(error) => Self::Internal(error),
        }
    }
}

/// What one provider does for links. [`Links::provider`] picks the implementation.
///
/// A `reference` is how a picker names a thing it listed: an issue key such as `ELY-12`, a
/// saved filter's id, `owner/name#12` for a GitHub issue, pull request, or milestone, and
/// `owner/name:label` for a label. Each implementation checks it before any call.
pub trait Provider: Send + Sync {
    /// The thing `reference` names, read through the credential, refused when the
    /// credential may not touch it or it is not a `kind`.
    fn find<'call>(
        &'call self,
        credential_id: Uuid,
        kind: LinkKind,
        reference: &'call str,
    ) -> BoxFuture<'call, Result<Remote, LinkError>>;

    /// A linked thing as it is now.
    fn read<'call>(
        &'call self,
        link: &'call ActionItemLink,
    ) -> BoxFuture<'call, Result<Remote, LinkError>>;

    /// The linked things the provider updated since `since`, all of them when `since` is
    /// `None`. `links` all go through the credential `credential_id`.
    fn changes<'call>(
        &'call self,
        credential_id: Uuid,
        links: &'call [ActionItemLink],
        since: Option<DateTime<Utc>>,
    ) -> BoxFuture<'call, Result<Vec<Remote>, LinkError>>;

    /// The container `reference` names, read through the credential.
    fn find_container<'call>(
        &'call self,
        credential_id: Uuid,
        kind: ContainerKind,
        reference: &'call str,
    ) -> BoxFuture<'call, Result<RemoteContainer, LinkError>>;

    /// A container's children as they are now.
    fn children<'call>(
        &'call self,
        container: &'call InitiativeLink,
    ) -> BoxFuture<'call, Result<Children, LinkError>>;

    /// Moves a linked issue to done, unless it is there already. The provider's current
    /// state is read first, so a retried write never moves an issue twice.
    fn close<'call>(
        &'call self,
        link: &'call ActionItemLink,
    ) -> BoxFuture<'call, Result<WriteOutcome, LinkError>>;

    /// Posts a comment to a linked thing, as the credential's account.
    fn comment<'call>(
        &'call self,
        link: &'call ActionItemLink,
        body: &'call str,
    ) -> BoxFuture<'call, Result<(), LinkError>>;
}

/// Every provider, and the signal that wakes the watcher. Cheap to clone; clones share the
/// providers and the signal.
#[derive(Clone)]
pub struct Links {
    jira: Arc<JiraProvider>,
    github: Arc<GithubProvider>,
    wake: Arc<Notify>,
}

impl std::fmt::Debug for Links {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Links").finish_non_exhaustive()
    }
}

impl Links {
    /// Links through `jira` and `github`, which share their HTTP clients with the callers'.
    pub fn new(database: Pool, cipher: Arc<Cipher>, jira: &Jira, github: &Github) -> Self {
        Self {
            jira: Arc::new(JiraProvider::new(
                jira.clone(),
                database.clone(),
                Arc::clone(&cipher),
            )),
            github: Arc::new(GithubProvider::new(github.clone(), database, cipher)),
            wake: Arc::new(Notify::new()),
        }
    }

    /// The implementation for `provider`. The one place a provider is matched on.
    pub fn provider(&self, provider: LinkProvider) -> &dyn Provider {
        match provider {
            LinkProvider::Jira => self.jira.as_ref(),
            LinkProvider::Github => self.github.as_ref(),
        }
    }

    /// Asks the watcher for a pass now rather than at its next interval, after a write that
    /// owes the provider something or links a container.
    pub fn wake_watcher(&self) {
        self.wake.notify_one();
    }

    /// Resolves when [`Links::wake_watcher`] is called. At most one wake is kept while the
    /// watcher is busy, since one pass serves every request made during it.
    pub async fn woken(&self) {
        self.wake.notified().await;
    }
}
