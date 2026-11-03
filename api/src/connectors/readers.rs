// Copyright © 2026 Jalapeno Labs

//! The reader for each kind read through a browser. See `docs/connectors.md`, Ingest.
//!
//! [`Reader`] is the one place that matches a browser-read kind to the code that reads it, so
//! `ingest` and the kinds' checks never do. A new browser-read kind adds a variant here, its
//! kind to [`ConnectorKind::BROWSER_READ`], and a row in [`for_kind`].

use super::slack::Slack;
use super::webmail::{self, Webmail};
use super::{BrowserReader, ConnectorError, ConnectorKind, ReadInput, Reading};
use crate::browser::devtools::Devtools;
use crate::models::browser_session::BrowserSession;

/// Reads one kind's account through its browser.
#[derive(Debug, Clone)]
pub enum Reader {
    Webmail(Webmail),
    Slack(Slack),
}

/// The reader for `kind`, or `None` for a kind that is not read through a browser.
pub fn for_kind(kind: ConnectorKind, devtools: &Devtools) -> Option<Reader> {
    match kind {
        ConnectorKind::Outlook | ConnectorKind::Gmail => webmail::service(kind)
            .map(|service| Reader::Webmail(Webmail::new(service, devtools.clone()))),
        ConnectorKind::Slack => Some(Reader::Slack(Slack::new(devtools.clone()))),
        ConnectorKind::Jira | ConnectorKind::Github | ConnectorKind::Fireflies => None,
    }
}

impl BrowserReader for Reader {
    fn kind(&self) -> ConnectorKind {
        match self {
            Self::Webmail(reader) => reader.kind(),
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
            Self::Slack(reader) => reader.read(browser, input).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_browser_read_kind_has_a_reader_and_no_other_kind_does() {
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
