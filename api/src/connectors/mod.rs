// Copyright © 2026 Jalapeno Labs

//! Connectors: services Elysium reads as its user, the accounts it reads them through, and the
//! timeline everything they read lands on. See `docs/connectors.md`.
//!
//! # Accounts and connectors
//!
//! An account is how Elysium signs in to a provider as a person ([`AccountKind`]): an
//! Atlassian or GitHub token, an API key, or a browser the person signed in to. A connector is
//! one use of one account (`crate::models::connector`): a [`ConnectorKind`], what it reads, and
//! what its reading has found. One account can feed several connectors, each kind once.
//!
//! # Kinds are defined once
//!
//! Each kind is one module under [`kinds`] that declares what the hub needs to know about it:
//! the account it reads through, what it reads, and its ordered checks ([`checks`]). The API
//! serves those definitions, and the web app renders its forms and check lists from them.
//!
//! # Reading
//!
//! [`ingest`] reads every active webmail connector on an interval, stores what it reads as
//! timeline records, raises an inbox item for each new message that wants attention, and
//! publishes `timelineRecord.created`, which is where Elysia's triage picks up. Meetings have
//! their own loop (`crate::meetings`), and Jira and GitHub are read by the link watcher
//! (`crate::action_items::watcher`), Confluence by its own reader (`crate::confluence::reader`).
//! [`activity`] is where each of them reports a sync.

pub mod accounts;
pub mod activity;
pub mod checks;
pub mod ingest;
pub mod kinds;
pub mod webmail;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use self::activity::Activity;
use self::checks::CheckRuns;
use self::ingest::Ingest;
use crate::models::browser_session::BrowserSession;

/// What request handlers reach the hub through. Cloning is cheap: each field is a handle.
#[derive(Debug, Clone)]
pub struct Connectors {
    /// Reads mailboxes; a sync someone asks for reads through it.
    pub ingest: Ingest,
    /// Reads Confluence spaces; a sync someone asks for, and Confluence's checks, go through it.
    pub confluence: crate::confluence::reader::Reader,
    /// What is syncing, and where syncs are recorded.
    pub activity: Activity,
    /// Check runs the web app is following.
    pub check_runs: CheckRuns,
}

/// Every kind of connector. Stored as text in `connectors.kind` and `timeline_records.source_kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorKind {
    /// Jira Cloud issues, read through an Atlassian account by the link watcher.
    Jira,
    /// GitHub issues and pull requests, read through a GitHub account by the link watcher.
    Github,
    Outlook,
    Gmail,
    /// Meetings recorded and transcribed by Fireflies.ai, read with its owner's API key. See
    /// `crate::meetings`.
    Fireflies,
    /// Pages and comments on a Confluence Cloud site, read through an Atlassian account a Jira
    /// connector already uses. See `crate::confluence`.
    Confluence,
}

impl ConnectorKind {
    /// Every kind, in the order the hub lists them.
    pub const ALL: [Self; 6] = [
        Self::Jira,
        Self::Confluence,
        Self::Github,
        Self::Outlook,
        Self::Gmail,
        Self::Fireflies,
    ];

    /// The kinds the link watcher reads (`crate::action_items::watcher`), which records its
    /// progress per account rather than on the connector. An Atlassian account can feed other
    /// kinds too, so anything about the watcher's reading asks for these kinds, never for an
    /// account alone.
    pub const LINKS: [Self; 2] = [Self::Jira, Self::Github];

    /// The kinds that read mail through a browser, which `ingest` reads.
    pub const WEBMAIL: [Self; 2] = [Self::Outlook, Self::Gmail];

    /// The kinds that read meetings, which `crate::meetings` reads.
    pub const MEETINGS: [Self; 1] = [Self::Fireflies];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Jira => "jira",
            Self::Github => "github",
            Self::Outlook => "outlook",
            Self::Gmail => "gmail",
            Self::Fireflies => "fireflies",
            Self::Confluence => "confluence",
        }
    }

    /// Whether the link watcher reads this kind. See [`ConnectorKind::LINKS`].
    pub fn is_link(self) -> bool {
        Self::LINKS.contains(&self)
    }

    /// The kind stored as `text`, if it is one.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

/// How an account signs in, which decides the store it lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    /// A site, an account email, and an API token (`jira_credentials`).
    Atlassian,
    /// A personal access token (`github_credentials`).
    Github,
    /// A browser the person signed in to (`browser_sessions`).
    Browser,
    /// A Fireflies API key (`token_accounts`).
    Fireflies,
}

impl AccountKind {
    pub const ALL: [Self; 4] = [
        Self::Atlassian,
        Self::Github,
        Self::Browser,
        Self::Fireflies,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Atlassian => "atlassian",
            Self::Github => "github",
            Self::Browser => "browser",
            Self::Fireflies => "fireflies",
        }
    }
}

/// Whose a connector is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Everyone in the workspace sees and manages it, as they do its account.
    Workspace,
    /// Its creator's alone: their mail, their meetings.
    Personal,
}

/// Whether the account was signed in when it was read. Stored as `connectors.sign_in_state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignInState {
    SignedIn,
    /// The service is asking its user to sign in again.
    SignedOut,
    /// The page could not be read, so whether it is signed in is not known.
    Unknown,
}

impl SignInState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SignedIn => "signed_in",
            Self::SignedOut => "signed_out",
            Self::Unknown => "unknown",
        }
    }
}

/// One thing a connector read: a message, for mail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    /// The provider's own id for it, unique within the account.
    pub external_id: String,
    /// When it happened, when the provider says; otherwise when it was read.
    pub occurred_at: Option<DateTime<Utc>>,
    /// Who it came from, as the provider shows them.
    pub actor: String,
    pub title: String,
    pub body: String,
    pub url: Option<String>,
    /// Whether it wants attention: an unread message raises an inbox item.
    pub wants_attention: bool,
    /// What else the record keeps, in its kind's own terms, such as a page's space and version.
    pub metadata: serde_json::Value,
}

/// What one read of an account found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub sign_in: SignInState,
    /// Newest first, as the provider lists them.
    pub items: Vec<Read>,
}

/// A connector could not read its account.
#[derive(Debug, thiserror::Error)]
pub enum ConnectorError {
    #[error(transparent)]
    Devtools(#[from] crate::browser::devtools::DevtoolsError),
    #[error("the page answered something the connector does not understand: {0}")]
    Unexpected(String),
}

/// A service Elysium reads through a browser its person signed in to.
pub trait BrowserReader {
    fn kind(&self) -> ConnectorKind;

    /// Reads what the account shows now, through the browser it is signed in to.
    fn read(
        &self,
        browser: &BrowserSession,
    ) -> impl Future<Output = Result<Reading, ConnectorError>> + Send;
}
