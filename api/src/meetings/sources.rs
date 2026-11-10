// Copyright © 2026 Jalapeno Labs

//! The meeting source behind each connector kind that provides meetings, and how each signs in.
//!
//! [`Sources::for_kind`] is the one place a kind is matched to its source, so the listing and
//! processing loop reads every source the same way and never names one. A connector kind
//! declares that it provides meetings (`kinds::Provides::Meetings`); a new source adds its
//! client here, a variant to [`Source`] and to [`SignedIn`], and its row in
//! [`Sources::for_kind`], and a test fails when a kind that provides meetings has no source.
//!
//! # Signing in
//!
//! Each source reads as its connector's person, signed in the way the connector signs in
//! ([`MeetingSource::SignIn`]), and [`Source::sign_in`] resolves that sign-in from the
//! connector beside the source: a key-based source opens the token account it reads through,
//! and a source read through a browser finds the browser its person signed in to. The loop and
//! the fetch step hold a [`SignedIn`] source and never handle a credential themselves.

use anyhow::Context;
use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use secrecy::SecretString;

use super::fireflies::Fireflies;
use super::fireflies_browser::FirefliesBrowser;
use super::source::{FetchedMeeting, ListedMeeting, MeetingSource, SourceError};
use crate::browser::devtools::Devtools;
use crate::connectors::ConnectorKind;
use crate::connectors::kinds::teams as teams_kind;
use crate::connectors::slack::meetings::{SlackMeetings, SlackSignIn};
use crate::connectors::slack::{self, Slack};
use crate::connectors::teams::meetings::TeamsMeetings;
use crate::crypto::Cipher;
use crate::models::browser_session::{self, BrowserSession};
use crate::models::connector::Connector;
use crate::models::token_account;

/// The client of every meeting source, each built once and shared by every connector of its
/// kind.
#[derive(Debug, Clone)]
pub struct Sources {
    fireflies: Fireflies,
    fireflies_browser: FirefliesBrowser,
    teams: TeamsMeetings,
    slack: SlackMeetings,
}

impl Sources {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            fireflies: Fireflies::new(http.clone()),
            fireflies_browser: FirefliesBrowser::new(Devtools::new(http.clone())),
            teams: TeamsMeetings::new(Devtools::new(http.clone())),
            slack: SlackMeetings::new(Slack::new(Devtools::new(http))),
        }
    }

    /// Every source read through `fireflies`, such as a client of a fake Fireflies.
    #[cfg(test)]
    pub fn with_fireflies(fireflies: Fireflies) -> Self {
        Self {
            fireflies,
            fireflies_browser: FirefliesBrowser::new(Devtools::new(reqwest::Client::new())),
            teams: TeamsMeetings::new(Devtools::new(reqwest::Client::new())),
            slack: SlackMeetings::new(Slack::new(Devtools::new(reqwest::Client::new()))),
        }
    }

    /// The Fireflies client, for the Fireflies kind's own checks and sign-in.
    pub fn fireflies(&self) -> &Fireflies {
        &self.fireflies
    }

    /// The client reading Fireflies through a browser, for that kind's own checks.
    pub fn fireflies_browser(&self) -> &FirefliesBrowser {
        &self.fireflies_browser
    }

    /// The Teams client, for the Teams kind's own checks.
    pub fn teams(&self) -> &TeamsMeetings {
        &self.teams
    }

    /// The Slack huddles client, for the Slack kind's own checks.
    pub fn slack(&self) -> &SlackMeetings {
        &self.slack
    }

    /// The source a connector of `kind` reads its meetings from, or `None` for a kind that
    /// provides no meetings.
    pub fn for_kind(&self, kind: ConnectorKind) -> Option<Source> {
        match kind {
            ConnectorKind::Fireflies => Some(Source::Fireflies(self.fireflies.clone())),
            ConnectorKind::FirefliesBrowser => {
                Some(Source::FirefliesBrowser(self.fireflies_browser.clone()))
            }
            ConnectorKind::Teams => Some(Source::Teams(self.teams.clone())),
            ConnectorKind::Slack => Some(Source::Slack(self.slack.clone())),
            ConnectorKind::Jira
            | ConnectorKind::Confluence
            | ConnectorKind::Github
            | ConnectorKind::Outlook
            | ConnectorKind::Gmail
            | ConnectorKind::Youtrack => None,
        }
    }

    /// Whether `connector` reads its meetings now ([`Source::reads`]); a kind that provides no
    /// meetings reads none.
    pub fn reads_meetings(&self, connector: &Connector) -> bool {
        ConnectorKind::parse(&connector.kind)
            .and_then(|kind| self.for_kind(kind))
            .is_some_and(|source| source.reads(connector))
    }

    /// `connector`'s source, signed in as its person.
    ///
    /// # Errors
    /// When the connector's kind provides no meetings, or its sign-in cannot be read.
    pub async fn signed_in(
        &self,
        connection: &mut AsyncPgConnection,
        cipher: &Cipher,
        connector: &Connector,
    ) -> anyhow::Result<SignedIn> {
        let source = ConnectorKind::parse(&connector.kind)
            .and_then(|kind| self.for_kind(kind))
            .with_context(|| format!("connector kind {:?} provides no meetings", connector.kind))?;
        source.sign_in(connection, cipher, connector).await
    }
}

/// One meeting source's client, before it is signed in as anyone.
#[derive(Debug, Clone)]
pub enum Source {
    Fireflies(Fireflies),
    FirefliesBrowser(FirefliesBrowser),
    Teams(TeamsMeetings),
    Slack(SlackMeetings),
}

impl Source {
    /// Whether `connector` reads its meetings now. Fireflies reads nothing else, so always,
    /// either way it signs in; a Teams connector also reads chats, and reads meetings only while
    /// its person leaves them on and once it has a storage location to keep their recordings in;
    /// a Slack connector reads huddles while its person leaves them on, and needs no storage
    /// location, since a huddle keeps no recording.
    pub fn reads(&self, connector: &Connector) -> bool {
        match self {
            Self::Fireflies(_) | Self::FirefliesBrowser(_) => true,
            Self::Teams(_client) => {
                teams_kind::reads_meetings(&connector.content, connector.storage_location_id)
            }
            Self::Slack(_client) => {
                slack::parse_content(&connector.content).is_ok_and(|content| content.read_meetings)
            }
        }
    }

    /// This source signed in as `connector`'s person, the way `connector` signs in.
    ///
    /// # Errors
    /// When the connector has no sign-in of the kind its source reads with, or it cannot be
    /// read or opened.
    pub async fn sign_in(
        self,
        connection: &mut AsyncPgConnection,
        cipher: &Cipher,
        connector: &Connector,
    ) -> anyhow::Result<SignedIn> {
        match self {
            Self::Fireflies(client) => {
                let key = key_of(connection, cipher, connector).await?;
                Ok(SignedIn::Fireflies(client, key))
            }
            Self::FirefliesBrowser(client) => {
                let browser = browser_of(connection, connector).await?;
                Ok(SignedIn::FirefliesBrowser(client, browser))
            }
            Self::Teams(client) => {
                let browser = browser_of(connection, connector).await?;
                Ok(SignedIn::Teams(client, browser))
            }
            Self::Slack(client) => {
                let browser = browser_of(connection, connector).await?;
                let content = slack::parse_content(&connector.content)?;
                Ok(SignedIn::Slack(client, SlackSignIn { browser, content }))
            }
        }
    }
}

/// One connector's meeting source, signed in as its person. An enum rather than a trait
/// object, since [`MeetingSource`]'s futures and sign-ins keep it from being one, and the set of
/// sources is known; each variant pairs a client with the sign-in that client reads with.
#[derive(Debug)]
pub enum SignedIn {
    Fireflies(Fireflies, SecretString),
    FirefliesBrowser(FirefliesBrowser, BrowserSession),
    Teams(TeamsMeetings, BrowserSession),
    Slack(SlackMeetings, SlackSignIn),
}

impl SignedIn {
    /// Meetings that started at or after `since`, newest first, at most `limit` of them.
    ///
    /// # Errors
    /// [`SourceError`] as the source answers.
    pub async fn list(
        &self,
        since: Option<DateTime<Utc>>,
        limit: u32,
    ) -> Result<Vec<ListedMeeting>, SourceError> {
        match self {
            Self::Fireflies(client, key) => client.list(key, since, limit).await,
            Self::FirefliesBrowser(client, browser) => client.list(browser, since, limit).await,
            Self::Teams(client, browser) => client.list(browser, since, limit).await,
            Self::Slack(client, sign_in) => client.list(sign_in, since, limit).await,
        }
    }

    /// One meeting's transcript and a fresh address to download its recording from.
    ///
    /// # Errors
    /// [`SourceError`] as the source answers.
    pub async fn fetch(&self, external_id: &str) -> Result<FetchedMeeting, SourceError> {
        match self {
            Self::Fireflies(client, key) => client.fetch(key, external_id).await,
            Self::FirefliesBrowser(client, browser) => client.fetch(browser, external_id).await,
            Self::Teams(client, browser) => client.fetch(browser, external_id).await,
            Self::Slack(client, sign_in) => client.fetch(sign_in, external_id).await,
        }
    }
}

/// The API key `connector` reads its meetings with, from the token account it reads through.
async fn key_of(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    connector: &Connector,
) -> anyhow::Result<SecretString> {
    let token_account_id = connector
        .token_account_id
        .context("this meeting connector reads with an API key, and it has none")?;
    let account = token_account::find(connection, token_account_id).await?;
    account
        .credential(cipher)
        .context("the connector's key cannot be decrypted")
}

/// The browser `connector` reads through, signed in to its service by its person.
async fn browser_of(
    connection: &mut AsyncPgConnection,
    connector: &Connector,
) -> anyhow::Result<BrowserSession> {
    let browser_session_id = connector
        .browser_session_id
        .context("this meeting connector is still waiting for its browser")?;
    browser_session::find(connection, browser_session_id)
        .await?
        .context("this meeting connector's browser was removed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::kinds::{self, Provides};

    #[test]
    fn every_kind_that_provides_meetings_has_a_source_and_no_other_does() {
        let sources = Sources::new(reqwest::Client::new());
        let providing = kinds::providing(Provides::Meetings);
        for kind in ConnectorKind::ALL {
            assert_eq!(
                sources.for_kind(kind).is_some(),
                providing.contains(&kind),
                "{} provides meetings exactly when it has a meeting source",
                kind.as_str()
            );
        }
    }
}
