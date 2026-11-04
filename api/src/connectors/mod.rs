// Copyright © 2026 Jalapeno Labs

//! Connectors: services Elysium reads as its user, each through an account of its own, and the
//! timeline everything they read lands on. See `docs/connectors.md`.
//!
//! # A connector owns its sign-in
//!
//! An account is how Elysium signs in to a provider as a person ([`AccountKind`]): an
//! Atlassian or GitHub token, an API key, or a browser the person signed in to. A connector
//! (`crate::models::connector`) is a [`ConnectorKind`], the one account it owns, what it reads,
//! and what its reading has found. No account is shared: adding a connector signs in, and
//! removing it removes the account.
//!
//! # Kinds are defined once
//!
//! Each kind is one module under [`kinds`] that declares what the hub needs to know about it:
//! the account it reads through, what it reads, and its ordered checks ([`checks`]). The API
//! serves those definitions, and the web app renders its forms and check lists from them.
//!
//! # Reading
//!
//! [`ingest`] reads every active connector read through a browser on an interval, with the
//! reader [`readers::for_kind`] answers for its kind, stores what it reads as
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
pub mod readers;
pub mod slack;
pub mod teams;
pub mod webmail;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use self::activity::Activity;
use self::checks::CheckRuns;
use self::ingest::Ingest;
use crate::models::browser_session::BrowserSession;
use crate::youtrack::reader::YoutrackReader;

/// What request handlers reach the hub through. Cloning is cheap: each field is a handle.
#[derive(Debug, Clone)]
pub struct Connectors {
    /// Reads mailboxes; a sync someone asks for reads through it.
    pub ingest: Ingest,
    /// Reads Confluence spaces; a sync someone asks for, and Confluence's checks, go through it.
    pub confluence: crate::confluence::reader::Reader,
    /// Reads YouTrack sites, and holds the client their checks and options call.
    pub youtrack: YoutrackReader,
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
    /// Microsoft Teams chats and channels, read from Teams' own store in a Microsoft browser.
    /// See [`teams`].
    Teams,
    /// Slack messages, read through Slack's web client in a browser its person signed in to.
    /// See [`slack`].
    Slack,
    /// Meetings recorded and transcribed by Fireflies.ai, read with its owner's API key. See
    /// `crate::meetings`.
    Fireflies,
    /// Pages and comments on a Confluence Cloud site, read through an Atlassian account of its
    /// own. See `crate::confluence`.
    Confluence,
    /// YouTrack Cloud issues and comments, read with its owner's permanent token. See
    /// `crate::youtrack`.
    Youtrack,
}

impl ConnectorKind {
    /// Every kind, in the order the hub lists them.
    pub const ALL: [Self; 9] = [
        Self::Jira,
        Self::Confluence,
        Self::Github,
        Self::Outlook,
        Self::Gmail,
        Self::Teams,
        Self::Slack,
        Self::Fireflies,
        Self::Youtrack,
    ];

    /// The kinds the link watcher reads (`crate::action_items::watcher`), which records its
    /// progress per account rather than on the connector. Confluence's account is an Atlassian
    /// one too, so anything about the watcher's reading asks for these kinds, never for an
    /// Atlassian account alone.
    pub const LINKS: [Self; 2] = [Self::Jira, Self::Github];

    /// Every kind read through a browser, which `ingest` reads through [`readers::for_kind`].
    pub const BROWSER_READ: [Self; 4] = [Self::Outlook, Self::Gmail, Self::Teams, Self::Slack];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Jira => "jira",
            Self::Github => "github",
            Self::Outlook => "outlook",
            Self::Gmail => "gmail",
            Self::Teams => "teams",
            Self::Slack => "slack",
            Self::Fireflies => "fireflies",
            Self::Confluence => "confluence",
            Self::Youtrack => "youtrack",
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
    /// A YouTrack permanent token and the site it was issued on (`token_accounts`).
    Youtrack,
}

impl AccountKind {
    pub const ALL: [Self; 5] = [
        Self::Atlassian,
        Self::Github,
        Self::Browser,
        Self::Fireflies,
        Self::Youtrack,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Atlassian => "atlassian",
            Self::Github => "github",
            Self::Browser => "browser",
            Self::Fireflies => "fireflies",
            Self::Youtrack => "youtrack",
        }
    }

    /// The kind stored as `text`, if it is one.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
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
    /// The reader's own state for its next read, such as a watermark, stored in the
    /// connector's cursor as `reader`. `None` keeps the state the read was given.
    pub state: Option<serde_json::Value>,
}

impl Reading {
    /// A read that found nothing, keeping the reader's state.
    pub const fn nothing(sign_in: SignInState) -> Self {
        Self {
            sign_in,
            items: Vec::new(),
            state: None,
        }
    }
}

/// What a browser reader is given besides its browser: what it kept from its last read, and
/// what its connector reads.
#[derive(Debug, Clone, Copy)]
pub struct ReadInput<'connector> {
    /// What the reader answered as [`Reading::state`] last time; `null` before its first read.
    pub state: &'connector serde_json::Value,
    /// The connector's content (`connectors.content`), in its kind's own terms.
    pub content: &'connector serde_json::Value,
}

/// A connector could not read its account, or could not send through it.
#[derive(Debug, thiserror::Error)]
pub enum ConnectorError {
    #[error(transparent)]
    Devtools(#[from] crate::browser::devtools::DevtoolsError),
    #[error("the page answered something the connector does not understand: {0}")]
    Unexpected(String),
    /// The service, or the page, would not do what was asked; the message says why, in words
    /// its person can act on, such as signing in again.
    #[error("{0}")]
    Refused(String),
}

/// A message a connector sends as its person.
#[derive(Debug, Clone, Copy)]
pub struct Outgoing<'draft> {
    /// Where it goes, in the kind's own terms, as `kinds::reply_target` wrote it.
    pub target: &'draft serde_json::Value,
    /// What it says, as the person wrote it: plain text.
    pub body: &'draft str,
    /// The person's name, for a service that shows one beside the message, such as Teams.
    pub sender_name: &'draft str,
}

/// The message a send posted, named the way the kind's reader names what it reads, so reading
/// it back later records nothing twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    /// The provider's own id for it, as [`Read::external_id`] would hold it.
    pub external_id: String,
    pub occurred_at: DateTime<Utc>,
    pub url: Option<String>,
    /// Where it was said, as a record the kind reads keeps it.
    pub metadata: serde_json::Value,
}

/// A service Elysium reads through a browser its person signed in to.
pub trait BrowserReader {
    fn kind(&self) -> ConnectorKind;

    /// Reads what the account shows now, through the browser it is signed in to, from where
    /// `input`'s state says the last read left off.
    fn read(
        &self,
        browser: &BrowserSession,
        input: ReadInput<'_>,
    ) -> impl Future<Output = Result<Reading, ConnectorError>> + Send;
}
