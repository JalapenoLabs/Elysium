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
//! [`ingest`] reads every active connector read through a browser on an interval, with the
//! reader [`readers::for_kind`] answers for its kind, stores what it reads as
//! timeline records, raises an inbox item for each new message that wants attention, and
//! publishes `timelineRecord.created`, which is where Elysia's triage picks up. Meetings have
//! their own loop (`crate::meetings`), and Jira and GitHub are read by the link watcher
//! (`crate::action_items::watcher`). [`activity`] is where each of them reports a sync.

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
    /// YouTrack Cloud issues and comments, read with its owner's permanent token. See
    /// `crate::youtrack`.
    Youtrack,
}

impl ConnectorKind {
    /// Every kind, in the order the hub lists them.
    pub const ALL: [Self; 8] = [
        Self::Jira,
        Self::Github,
        Self::Outlook,
        Self::Gmail,
        Self::Teams,
        Self::Slack,
        Self::Fireflies,
        Self::Youtrack,
    ];

    /// Every kind read through a browser, which `ingest` reads through [`readers::for_kind`].
    pub const BROWSER_READ: [Self; 4] = [Self::Outlook, Self::Gmail, Self::Teams, Self::Slack];

    /// The kinds that read meetings, which `crate::meetings` reads.
    pub const MEETINGS: [Self; 1] = [Self::Fireflies];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Jira => "jira",
            Self::Github => "github",
            Self::Outlook => "outlook",
            Self::Gmail => "gmail",
            Self::Teams => "teams",
            Self::Slack => "slack",
            Self::Fireflies => "fireflies",
            Self::Youtrack => "youtrack",
        }
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

    /// Reads what the account shows now, through the browser it is signed in to, from where
    /// `input`'s state says the last read left off.
    fn read(
        &self,
        browser: &BrowserSession,
        input: ReadInput<'_>,
    ) -> impl Future<Output = Result<Reading, ConnectorError>> + Send;
}
