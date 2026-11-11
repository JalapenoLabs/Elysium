// Copyright © 2026 Jalapeno Labs

//! Keeps the sign-ins in connectors' browsers from going stale: once a day each browser's
//! service is loaded again, so its rolling session renews, and its connector is read right after,
//! so a session that ended anyway is recorded as signed out. See `docs/browser.md`, Browser
//! sessions.
//!
//! Outlook, Teams, and Slack keep a person signed in for as long as the person keeps using them,
//! by renewing the session's cookies whenever the app loads. A browser that only Elysium reads
//! never loads anything again after its sign-in: a single-page app keeps running, and its reader
//! keeps reading what the page holds, until the session behind it lapses unnoticed.
//!
//! [`due`] decides which browser is refreshed: at most one per ingest pass, the one longest
//! without, once [`REFRESH_INTERVAL`] has passed since its last refresh or its sign-in. One a
//! minute keeps eight browsers apart however they started, and the last refresh is stored on the
//! row (`browser_sessions.refreshed_at`), so a restart neither reloads every browser at once nor
//! forgets one. [`refresh`] does it, and the ingest pass reads the connector straight after.
//!
//! This runs in the ingest pass, not the browser reconciler: the reconciler is a Docker pass every
//! ten seconds under Docker's lock, and the read that tells whether the reload found the service
//! signed in is the pass's own.

use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use url::Url;

use super::readers::{ServicePage, service_page};
use super::{ConnectorError, ConnectorKind};
use crate::browser::BrowserState;
use crate::browser::devtools::{Devtools, Tab};
use crate::models::browser_session::BrowserSession;

/// How long a browser goes between refreshes. A day keeps well inside the shortest rolling
/// session these services keep for an idle sign-in, and is still rarely enough that a reload is
/// unlikely to land while its person is using the page.
pub const REFRESH_INTERVAL: TimeDelta = TimeDelta::hours(24);

/// How long a loaded page is given to start up before it is read. Teams and Outlook load their
/// shell first and the person's data a few seconds later; a read before then would report the
/// page as not signed in. A reading that comes too early is corrected by the next minute's read.
const SETTLE_DELAY: Duration = Duration::from_secs(20);

/// What a refresh did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refreshed {
    /// Reloaded the tab showing the service.
    Reloaded,
    /// No tab showed the service, so its start page was opened in a new one.
    Opened,
    /// A tab is asking to sign in already, so there was no session to renew.
    SigningIn,
}

impl Refreshed {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reloaded => "reloaded",
            Self::Opened => "opened",
            Self::SigningIn => "signing_in",
        }
    }
}

/// The browser most overdue a refresh among `browsers` at `now`, if any is due.
///
/// A browser is due once [`REFRESH_INTERVAL`] has passed since the later of its last refresh and
/// when its start page was opened, so one just added or reset, whose person may be signing in
/// on it, waits a day. One that is not running, or whose start page is not open yet, is not due
/// until it is.
pub fn due(browsers: &[BrowserSession], now: DateTime<Utc>) -> Option<&BrowserSession> {
    browsers
        .iter()
        .filter(|browser| browser.state == BrowserState::Running)
        .filter_map(|browser| {
            let opened = browser.start_page_opened_at?;
            let since = browser
                .refreshed_at
                .map_or(opened, |refreshed| refreshed.max(opened));
            Some((since, browser))
        })
        .filter(|(since, _browser)| *since + REFRESH_INTERVAL <= now)
        .min_by_key(|(since, _browser)| *since)
        .map(|(_since, browser)| browser)
}

/// Loads `kind`'s service again in `browser`, and answers once the page has had time to start.
///
/// The tab already showing the service is reloaded in place rather than replaced by a fresh one:
/// renewing the session needs nothing more than the app loading with its cookies, which a reload
/// does as a new tab would, and a reload keeps the person's tab where it is, steals no focus
/// from the window they may be watching, and makes that tab, the one its reader reads, show
/// whether the session still holds. A fresh tab would leave the old page reading what it held,
/// and closing a tab risks Chromium's exit with its last window. With no tab on the service, its
/// start page is opened in a new one, as the readers of Teams and Slack do, so a browser whose tab
/// was closed or navigated away is signed in to again rather than left to lapse.
///
/// # Errors
/// [`ConnectorError`] when the browser does not answer, or the page does not load in time.
pub async fn refresh(
    devtools: &Devtools,
    kind: ConnectorKind,
    browser: &BrowserSession,
) -> Result<Refreshed, ConnectorError> {
    let port = u16::try_from(browser.port).map_err(|_overflow| {
        ConnectorError::Unexpected("a browser port out of range".to_owned())
    })?;
    let tabs = devtools.tabs(port).await?;
    let refreshed = match find_service(kind, &tabs) {
        Found::App(tab) => {
            devtools.reload(port, tab).await?;
            Refreshed::Reloaded
        }
        Found::SignIn => return Ok(Refreshed::SigningIn),
        Found::Nothing => {
            let start_url = Url::parse(&browser.start_url).map_err(|_invalid| {
                ConnectorError::Unexpected(
                    "the browser's stored start page is not a URL".to_owned(),
                )
            })?;
            devtools.open_tab(port, &start_url).await?;
            Refreshed::Opened
        }
    };
    tokio::time::sleep(SETTLE_DELAY).await;
    Ok(refreshed)
}

/// What the browser's tabs show of the service.
enum Found<'tabs> {
    App(&'tabs Tab),
    SignIn,
    Nothing,
}

/// The first tab showing the service itself, else whether any asks to sign in to it.
fn find_service(kind: ConnectorKind, tabs: &[Tab]) -> Found<'_> {
    let mut found = Found::Nothing;
    for tab in tabs {
        match service_page(kind, &tab.url) {
            Some(ServicePage::App) => return Found::App(tab),
            Some(ServicePage::SignIn) => found = Found::SignIn,
            None => {}
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-10T12:00:00Z")
            .expect("a static timestamp parses")
            .with_timezone(&Utc)
    }

    /// A running browser whose start page opened, and which was last refreshed, so many hours
    /// before [`now`].
    fn browser(opened_hours_ago: Option<i64>, refreshed_hours_ago: Option<i64>) -> BrowserSession {
        let hours_ago = |hours: Option<i64>| hours.map(|hours| now() - TimeDelta::hours(hours));
        BrowserSession {
            id: Uuid::now_v7(),
            name: "slack".to_owned(),
            start_url: "https://app.slack.com/client".to_owned(),
            start_page_opened_at: hours_ago(opened_hours_ago),
            port: 59000,
            member_password_encrypted: Vec::new(),
            created_by: Uuid::now_v7(),
            created_at: now() - TimeDelta::days(30),
            updated_at: now(),
            state: BrowserState::Running,
            state_error: None,
            refreshed_at: hours_ago(refreshed_hours_ago),
        }
    }

    fn tab(url: &str) -> Tab {
        Tab {
            id: url.to_owned(),
            kind: "page".to_owned(),
            url: url.to_owned(),
            web_socket_debugger_url: None,
        }
    }

    #[test]
    fn nothing_is_due_within_a_day_of_its_last_refresh() {
        let browsers = [browser(Some(100), Some(23)), browser(Some(2), None)];
        assert!(due(&browsers, now()).is_none());
    }

    #[test]
    fn a_browser_never_refreshed_is_due_a_day_after_its_start_page_opened() {
        let browsers = [browser(Some(24), None)];
        assert_eq!(
            due(&browsers, now()).map(|due| due.id),
            Some(browsers[0].id)
        );
    }

    // Every browser is due after a deploy or a long outage; refreshing the most overdue one per
    // pass is what keeps them from all reloading in the same minute.
    #[test]
    fn only_the_most_overdue_browser_is_due_at_once() {
        let browsers = [
            browser(Some(200), Some(30)),
            browser(Some(200), Some(50)),
            browser(Some(40), None),
        ];
        assert_eq!(
            due(&browsers, now()).map(|due| due.id),
            Some(browsers[1].id)
        );
    }

    // A reset opens the start page again for a fresh sign-in, which a reload must not interrupt
    // however long ago the browser was last refreshed.
    #[test]
    fn a_browser_signed_in_again_since_its_refresh_waits_a_day_from_then() {
        let browsers = [browser(Some(1), Some(48))];
        assert!(due(&browsers, now()).is_none());
    }

    #[test]
    fn a_browser_not_running_or_never_opened_is_never_due() {
        let mut failed = browser(Some(100), None);
        failed.state = BrowserState::Failed;
        let mut starting = browser(Some(100), None);
        starting.state = BrowserState::Starting;
        let unopened = browser(None, Some(100));
        assert!(due(&[failed, starting, unopened], now()).is_none());
    }

    #[test]
    fn the_service_tab_is_reloaded_before_any_sign_in_tab_counts() {
        let tabs = [
            tab("chrome://newtab/"),
            tab("https://login.microsoftonline.com/common/login"),
            tab("https://teams.cloud.microsoft/v2/"),
        ];
        assert!(matches!(
            find_service(ConnectorKind::Teams, &tabs),
            Found::App(found) if found.url == "https://teams.cloud.microsoft/v2/"
        ));
        assert!(matches!(
            find_service(ConnectorKind::Teams, &tabs[..2]),
            Found::SignIn
        ));
        assert!(matches!(
            find_service(ConnectorKind::Outlook, &tabs[..1]),
            Found::Nothing
        ));
    }
}
