// Copyright © 2026 Jalapeno Labs

//! The meeting source behind each connector kind that provides meetings, and how each signs in.
//!
//! [`Sources::for_kind`] is the one place a kind is matched to its source, so the listing and
//! processing loop reads every source the same way and never names one. A connector kind
//! declares that it provides meetings (`kinds::Provides::Meetings`); a new source adds its
//! client here, a variant to [`Source`], and its row in [`Sources::for_kind`], and a test fails
//! when a kind that provides meetings has no source.
//!
//! [`sign_in_of`] resolves what a connector signs in with from the account it owns: a key
//! source's token account, or a browser source's browser. A source is handed only the sign-in
//! its own client reads as; a connector whose account is the other kind is refused.

use anyhow::Context;
use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;

use super::fireflies::Fireflies;
use super::fireflies_browser::FirefliesBrowser;
use super::source::{FetchedMeeting, ListedMeeting, MeetingSource, SignIn, SourceError};
use crate::browser::devtools::Devtools;
use crate::connectors::ConnectorKind;
use crate::crypto::Cipher;
use crate::models::browser_session;
use crate::models::connector::{AccountRef, Connector};
use crate::models::token_account;

/// The client of every meeting source, each built once and shared by every connector of its
/// kind.
#[derive(Debug, Clone)]
pub struct Sources {
    fireflies: Fireflies,
    fireflies_browser: FirefliesBrowser,
}

impl Sources {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            fireflies: Fireflies::new(http.clone()),
            fireflies_browser: FirefliesBrowser::new(Devtools::new(http)),
        }
    }

    /// Every source read through `fireflies`, such as a client of a fake Fireflies.
    #[cfg(test)]
    pub fn with_fireflies(fireflies: Fireflies) -> Self {
        Self {
            fireflies,
            fireflies_browser: FirefliesBrowser::new(Devtools::new(reqwest::Client::new())),
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

    /// The source a connector of `kind` reads its meetings from, or `None` for a kind that
    /// provides no meetings.
    pub fn for_kind(&self, kind: ConnectorKind) -> Option<Source> {
        match kind {
            ConnectorKind::Fireflies => Some(Source::Fireflies(self.fireflies.clone())),
            ConnectorKind::FirefliesBrowser => {
                Some(Source::FirefliesBrowser(self.fireflies_browser.clone()))
            }
            ConnectorKind::Jira
            | ConnectorKind::Confluence
            | ConnectorKind::Github
            | ConnectorKind::Outlook
            | ConnectorKind::Gmail
            | ConnectorKind::Teams
            | ConnectorKind::Slack
            | ConnectorKind::Youtrack => None,
        }
    }
}

/// What `connector` signs in to its meeting source with: the key its token account holds, or
/// the browser it owns.
///
/// # Errors
/// When the connector owns neither, such as one still waiting for its browser, when its browser
/// was removed, or when its key cannot be decrypted.
pub async fn sign_in_of(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    connector: &Connector,
) -> anyhow::Result<SignIn> {
    match connector.account() {
        Some(AccountRef::Token(id)) => {
            let account = token_account::find(connection, id).await?;
            let key = account
                .credential(cipher)
                .context("the connector's key cannot be decrypted")?;
            Ok(SignIn::Key(key))
        }
        Some(AccountRef::Browser(id)) => {
            let browser = browser_session::find(connection, id)
                .await?
                .context("the connector's browser was removed")?;
            Ok(SignIn::Browser(browser))
        }
        Some(AccountRef::Atlassian(_) | AccountRef::Github(_)) => Err(anyhow::anyhow!(
            "a meeting connector signs in with a key or a browser, and this one has neither"
        )),
        None => Err(anyhow::anyhow!(
            "the connector has no browser yet; start one from its page and sign in"
        )),
    }
}

/// One meeting source. An enum rather than a trait object, since [`MeetingSource`]'s futures
/// keep it from being one, and the set of sources is known.
#[derive(Debug, Clone)]
pub enum Source {
    Fireflies(Fireflies),
    FirefliesBrowser(FirefliesBrowser),
}

impl MeetingSource for Source {
    /// Whatever the connector signs in with; each source takes only its own kind of sign-in.
    type SignIn = SignIn;

    async fn list(
        &self,
        sign_in: &SignIn,
        since: Option<DateTime<Utc>>,
        limit: u32,
    ) -> Result<Vec<ListedMeeting>, SourceError> {
        match (self, sign_in) {
            (Self::Fireflies(source), SignIn::Key(key)) => source.list(key, since, limit).await,
            (Self::FirefliesBrowser(source), SignIn::Browser(browser)) => {
                source.list(browser, since, limit).await
            }
            (source, _other) => Err(mismatched(source)),
        }
    }

    async fn fetch(
        &self,
        sign_in: &SignIn,
        external_id: &str,
    ) -> Result<FetchedMeeting, SourceError> {
        match (self, sign_in) {
            (Self::Fireflies(source), SignIn::Key(key)) => source.fetch(key, external_id).await,
            (Self::FirefliesBrowser(source), SignIn::Browser(browser)) => {
                source.fetch(browser, external_id).await
            }
            (source, _other) => Err(mismatched(source)),
        }
    }
}

/// A connector whose account is not what its kind's source reads as, which only a database
/// written by hand holds.
fn mismatched(source: &Source) -> SourceError {
    let (name, reads_with) = match source {
        Source::Fireflies(_) => ("Fireflies", "an API key"),
        Source::FirefliesBrowser(_) => ("Fireflies (browser)", "a browser"),
    };
    SourceError::Unauthorized(format!(
        "{name} reads with {reads_with}, and this connector signs in otherwise; remove it and \
         add it again"
    ))
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

    #[tokio::test]
    async fn a_source_refuses_a_sign_in_of_the_other_kind() {
        let sources = Sources::new(reqwest::Client::new());
        let source = sources
            .for_kind(ConnectorKind::FirefliesBrowser)
            .expect("the browser kind has a source");

        let listed = source
            .list(&SignIn::Key(secrecy::SecretString::from("key")), None, 1)
            .await;

        assert!(
            matches!(&listed, Err(SourceError::Unauthorized(message)) if message.contains("reads with a browser")),
            "a browser source handed a key is refused: {listed:?}"
        );
    }
}
