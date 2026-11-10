// Copyright © 2026 Jalapeno Labs

//! The reader for each kind read through a browser. [`for_kind`] is the one place a kind is
//! matched to its reader, so `ingest` reads every browser kind the same way, and
//! [`service_page`] the one place a tab is matched to the service it shows.
//!
//! A new browser kind adds a variant here, its rows in [`for_kind`] and [`service_page`], and
//! itself to [`ConnectorKind::BROWSER_READ`]; a test fails when a listed kind has no reader, or
//! its start page is not its service.

use super::slack::{self, Slack};
use super::teams::{self, TEAMS_ORIGINS, Teams};
use super::webmail::{Webmail, service};
use super::{BrowserReader, ConnectorError, ConnectorKind, ReadInput, Reading};
use crate::browser::devtools::Devtools;
use crate::models::browser_session::BrowserSession;

/// What a tab shows of a kind's service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServicePage {
    /// The service itself, signed in: the page its reader reads.
    App,
    /// Where the service asks its person to sign in.
    SignIn,
}

/// What a tab at `url` shows of `kind`'s service, by the same addresses its reader goes by, or
/// `None` for a page that is not the service's, or a kind not read through a browser.
pub fn service_page(kind: ConnectorKind, url: &str) -> Option<ServicePage> {
    let starts_with_any = |origins: &[&str]| origins.iter().any(|origin| url.starts_with(origin));
    let (on_app, on_sign_in) = match kind {
        ConnectorKind::Outlook | ConnectorKind::Gmail => {
            let service = service(kind)?;
            (
                starts_with_any(service.mailbox_origins),
                starts_with_any(service.sign_in_origins),
            )
        }
        ConnectorKind::Teams => (
            starts_with_any(TEAMS_ORIGINS),
            starts_with_any(teams::SIGN_IN_ORIGINS),
        ),
        // The web client's own host is under slack.com too, so it is matched first.
        ConnectorKind::Slack => (starts_with_any(slack::APP_ORIGINS), slack::is_sign_in(url)),
        ConnectorKind::Jira
        | ConnectorKind::Confluence
        | ConnectorKind::Github
        | ConnectorKind::Fireflies
        | ConnectorKind::Youtrack => return None,
    };
    if on_app {
        return Some(ServicePage::App);
    }
    on_sign_in.then_some(ServicePage::SignIn)
}

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

    // The daily refresh reloads the tab showing a kind's service, and opens its start page when
    // none does; a start page that did not count as the service would be opened again each day.
    #[test]
    fn every_browser_kinds_start_page_is_its_service() {
        for kind in ConnectorKind::BROWSER_READ {
            let start_url = super::super::kinds::definition(kind)
                .start_url
                .expect("a browser kind names its start page");
            assert_eq!(
                service_page(kind, start_url),
                Some(ServicePage::App),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_sign_in_page_is_told_from_the_service_and_from_other_pages() {
        let cases = [
            (
                ConnectorKind::Outlook,
                "https://login.microsoftonline.com/common/oauth2",
                Some(ServicePage::SignIn),
            ),
            (
                ConnectorKind::Teams,
                "https://login.live.com/login.srf",
                Some(ServicePage::SignIn),
            ),
            (
                ConnectorKind::Gmail,
                "https://accounts.google.com/v3/signin",
                Some(ServicePage::SignIn),
            ),
            (
                ConnectorKind::Slack,
                "https://app.slack.com/client/T01/C02",
                Some(ServicePage::App),
            ),
            (
                ConnectorKind::Slack,
                "https://mooreslab.slack.com/signin",
                Some(ServicePage::SignIn),
            ),
            (ConnectorKind::Outlook, "chrome://newtab/", None),
            (
                ConnectorKind::Gmail,
                "https://outlook.office.com/mail/",
                None,
            ),
            (ConnectorKind::Jira, "https://example.atlassian.net/", None),
        ];
        for (kind, url, expected) in cases {
            assert_eq!(service_page(kind, url), expected, "{kind:?} at {url}");
        }
    }
}
