// Copyright © 2026 Jalapeno Labs

//! The meeting source behind each connector kind that provides meetings.
//!
//! [`Sources::for_kind`] is the one place a kind is matched to its source, so the listing and
//! processing loop reads every source the same way and never names one. A connector kind
//! declares that it provides meetings (`kinds::Provides::Meetings`); a new source adds its
//! client here, a variant to [`Source`], and its row in [`Sources::for_kind`], and a test fails
//! when a kind that provides meetings has no source.

use chrono::{DateTime, Utc};
use secrecy::SecretString;

use super::fireflies::Fireflies;
use super::source::{FetchedMeeting, ListedMeeting, MeetingSource, SourceError};
use crate::connectors::ConnectorKind;

/// The client of every meeting source, each built once and shared by every connector of its
/// kind.
#[derive(Debug, Clone)]
pub struct Sources {
    fireflies: Fireflies,
}

impl Sources {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            fireflies: Fireflies::new(http),
        }
    }

    /// The Fireflies client, for the Fireflies kind's own checks and sign-in.
    pub fn fireflies(&self) -> &Fireflies {
        &self.fireflies
    }

    /// The source a connector of `kind` reads its meetings from, or `None` for a kind that
    /// provides no meetings.
    pub fn for_kind(&self, kind: ConnectorKind) -> Option<Source> {
        match kind {
            ConnectorKind::Fireflies => Some(Source::Fireflies(self.fireflies.clone())),
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

/// One meeting source. An enum rather than a trait object, since [`MeetingSource`]'s futures
/// keep it from being one, and the set of sources is known.
#[derive(Debug, Clone)]
pub enum Source {
    Fireflies(Fireflies),
}

impl MeetingSource for Source {
    async fn list(
        &self,
        credential: &SecretString,
        since: Option<DateTime<Utc>>,
        limit: u32,
    ) -> Result<Vec<ListedMeeting>, SourceError> {
        match self {
            Self::Fireflies(source) => source.list(credential, since, limit).await,
        }
    }

    async fn fetch(
        &self,
        credential: &SecretString,
        external_id: &str,
    ) -> Result<FetchedMeeting, SourceError> {
        match self {
            Self::Fireflies(source) => source.fetch(credential, external_id).await,
        }
    }
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
