// Copyright © 2026 Jalapeno Labs

//! The reader for each kind read through a browser. [`for_kind`] is the one place a kind is
//! matched to its reader, so `ingest` reads every browser kind the same way.
//!
//! A new browser kind adds a variant here, its row in [`for_kind`], and itself to
//! [`ConnectorKind::BROWSER_READ`]; a test fails when a listed kind has no reader.

use super::slack::Slack;
use super::teams::Teams;
use super::webmail::{Webmail, service};
use super::{BrowserReader, ConnectorError, ConnectorKind, ReadInput, Reading};
use crate::browser::devtools::Devtools;
use crate::models::browser_session::BrowserSession;

/// A reader for one kind read through a browser.
#[derive(Debug, Clone)]
pub enum Reader {
    Webmail(Webmail),
    Teams(Teams),
    Slack(Slack),
}

/// The reader for `kind`, or `None` for a kind not read through a browser.
pub fn for_kind(kind: ConnectorKind, devtools: &Devtools) -> Option<Reader> {
    match kind {
        ConnectorKind::Outlook | ConnectorKind::Gmail => {
            service(kind).map(|service| Reader::Webmail(Webmail::new(service, devtools.clone())))
        }
        ConnectorKind::Teams => Some(Reader::Teams(Teams::new(devtools.clone()))),
        ConnectorKind::Slack => Some(Reader::Slack(Slack::new(devtools.clone()))),
        ConnectorKind::Jira
        | ConnectorKind::Confluence
        | ConnectorKind::Github
        | ConnectorKind::Fireflies
        | ConnectorKind::FirefliesBrowser
        | ConnectorKind::Youtrack => None,
    }
}

impl BrowserReader for Reader {
    fn kind(&self) -> ConnectorKind {
        match self {
            Self::Webmail(reader) => reader.kind(),
            Self::Teams(reader) => reader.kind(),
            Self::Slack(reader) => reader.kind(),
        }
    }

    async fn read(
        &self,
        browser: &BrowserSession,
        input: ReadInput<'_>,
    ) -> Result<Reading, ConnectorError> {
        match self {
            Self::Webmail(reader) => reader.read(browser, input).await,
            Self::Teams(reader) => reader.read(browser, input).await,
            Self::Slack(reader) => reader.read(browser, input).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_browser_kind_has_a_reader_of_that_kind_and_no_other_kind_does() {
        let devtools = Devtools::new(reqwest::Client::new());
        for kind in ConnectorKind::ALL {
            let reader = for_kind(kind, &devtools);
            assert_eq!(
                reader.is_some(),
                ConnectorKind::BROWSER_READ.contains(&kind),
                "{kind:?}"
            );
            if let Some(reader) = reader {
                assert_eq!(reader.kind(), kind);
            }
        }
    }
}
