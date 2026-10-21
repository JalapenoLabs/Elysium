// Copyright © 2026 Jalapeno Labs

//! Connectors: services Elysium reads as its user, and the timeline everything they read
//! lands on. See `docs/connectors.md`.
//!
//! Every connector fills one trait, [`Connector`]: it says whether its account is signed in,
//! and reads what is new. Today's connectors read a signed-in page in one of the person's
//! browsers ([`webmail`]); a connector on an API or a CLI fills the same trait. How it signs in
//! and stays signed in belongs to what it reads through: a browser keeps its own session.
//!
//! [`ingest`] runs every active account on an interval, stores what it reads as timeline
//! records, raises an inbox item for each new message that wants attention, and publishes
//! `timelineRecord.created`, which is where Elysia's triage picks up.

pub mod ingest;
pub mod webmail;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::models::browser_session::BrowserSession;

/// Every connector. Stored as text in `connector_accounts.kind` and `timeline_records.source_kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorKind {
    Outlook,
}

impl ConnectorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Outlook => "outlook",
        }
    }

    /// The kind stored as `text`, if it is one.
    pub fn parse(text: &str) -> Option<Self> {
        [Self::Outlook]
            .into_iter()
            .find(|kind| kind.as_str() == text)
    }
}

/// Whether the account was signed in when it was read. Stored as `connector_accounts.sign_in_state`.
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
}

/// A connector could not read its account.
#[derive(Debug, thiserror::Error)]
pub enum ConnectorError {
    #[error(transparent)]
    Devtools(#[from] crate::browser::devtools::DevtoolsError),
    #[error("the page answered something the connector does not understand: {0}")]
    Unexpected(String),
}

/// A service Elysium reads as its user.
pub trait Connector {
    fn kind(&self) -> ConnectorKind;

    /// Reads what the account shows now, through the browser it is signed in to.
    fn read(
        &self,
        browser: &BrowserSession,
    ) -> impl Future<Output = Result<Reading, ConnectorError>> + Send;
}
