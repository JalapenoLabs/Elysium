// Copyright © 2026 Jalapeno Labs

//! Mail read from a webmail page the person is signed in to, in their own browser.
//!
//! Each service is one row of [`SERVICES`]: the addresses its mailbox and its sign-in pages
//! live at, and the script that reads the message list from the page (`outlook.js`, `gmail.js`). The
//! script runs as the page, over `DevTools`, and answers
//! `{ signedIn, messages: [{ id, from, subject, preview, unread, receivedAt?, url? }] }`.
//! Reading never opens a message, so nothing is marked read.
//!
//! The check is cheap and deterministic: one evaluation per read, no model involved. Reading
//! a message closely, or acting on it, is Elysia's to do through the same browser.

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{Connector, ConnectorError, ConnectorKind, Read, Reading, SignInState};
use crate::browser::devtools::{Devtools, DevtoolsError};
use crate::models::browser_session::BrowserSession;

/// A webmail service the connector reads.
#[derive(Debug)]
pub struct Service {
    pub kind: ConnectorKind,
    /// Where its mailbox is: a tab at one of these is read.
    pub mailbox_origins: &'static [&'static str],
    /// Where it asks its user to sign in: a tab at one of these means the session ended.
    pub sign_in_origins: &'static [&'static str],
    pub script: &'static str,
}

pub const SERVICES: [Service; 2] = [
    Service {
        kind: ConnectorKind::Outlook,
        mailbox_origins: &[
            "https://outlook.office.com/mail",
            "https://outlook.office365.com/mail",
            "https://outlook.live.com/mail",
        ],
        sign_in_origins: &[
            "https://login.microsoftonline.com/",
            "https://login.live.com/",
        ],
        script: include_str!("outlook.js"),
    },
    Service {
        kind: ConnectorKind::Gmail,
        mailbox_origins: &["https://mail.google.com/mail"],
        sign_in_origins: &["https://accounts.google.com/"],
        script: include_str!("gmail.js"),
    },
];

/// The webmail service a connector kind reads, or `None` for a kind that reads something
/// else, such as meetings.
pub fn service(kind: ConnectorKind) -> Option<&'static Service> {
    SERVICES.iter().find(|service| service.kind == kind)
}

#[derive(Debug, Clone)]
pub struct Webmail {
    service: &'static Service,
    devtools: Devtools,
}

impl Webmail {
    pub const fn new(service: &'static Service, devtools: Devtools) -> Self {
        Self { service, devtools }
    }
}

impl Connector for Webmail {
    fn kind(&self) -> ConnectorKind {
        self.service.kind
    }

    async fn read(&self, browser: &BrowserSession) -> Result<Reading, ConnectorError> {
        let port = u16::try_from(browser.port).map_err(|_overflow| {
            ConnectorError::Unexpected("a browser port out of range".to_owned())
        })?;
        let tabs = self.devtools.tabs(port).await?;
        let on_mailbox = tabs
            .iter()
            .any(|tab| starts_with_any(&tab.url, self.service.mailbox_origins));
        let on_sign_in = tabs
            .iter()
            .any(|tab| starts_with_any(&tab.url, self.service.sign_in_origins));
        if !on_mailbox {
            let sign_in = if on_sign_in {
                SignInState::SignedOut
            } else {
                SignInState::Unknown
            };
            return Ok(Reading {
                sign_in,
                items: Vec::new(),
            });
        }

        let value = match self
            .devtools
            .evaluate(port, self.service.mailbox_origins, self.service.script)
            .await
        {
            Ok(value) => value,
            Err(DevtoolsError::NoTab(_origins)) => {
                return Ok(Reading {
                    sign_in: SignInState::Unknown,
                    items: Vec::new(),
                });
            }
            Err(error) => return Err(error.into()),
        };
        parse_page(&value)
    }
}

fn starts_with_any(url: &str, origins: &[&str]) -> bool {
    origins.iter().any(|origin| url.starts_with(origin))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    signed_in: bool,
    messages: Vec<PageMessage>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageMessage {
    id: String,
    #[serde(default)]
    from: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    preview: String,
    #[serde(default)]
    unread: bool,
    /// RFC 3339; one that does not parse is treated as absent, so one odd date never fails a read.
    received_at: Option<String>,
    url: Option<String>,
}

/// The longest preview kept on a record; the message itself stays in the mailbox.
const PREVIEW_MAX_CHARACTERS: usize = 1000;
/// Matches `timeline_records`' checks on the columns these fill.
const ACTOR_MAX_CHARACTERS: usize = 512;
const TITLE_MAX_CHARACTERS: usize = 2048;

/// What the page script answered, as a reading.
fn parse_page(value: &serde_json::Value) -> Result<Reading, ConnectorError> {
    let page: Page = serde_json::from_value(value.clone())
        .map_err(|error| ConnectorError::Unexpected(error.to_string()))?;
    if !page.signed_in {
        return Ok(Reading {
            sign_in: SignInState::SignedOut,
            items: Vec::new(),
        });
    }

    let items = page
        .messages
        .into_iter()
        .filter(|message| !message.id.trim().is_empty())
        .map(|message| Read {
            external_id: message.id,
            occurred_at: message
                .received_at
                .as_deref()
                .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
                .map(|at| at.with_timezone(&Utc)),
            actor: truncate(&message.from, ACTOR_MAX_CHARACTERS),
            title: truncate(&message.subject, TITLE_MAX_CHARACTERS),
            body: truncate(&message.preview, PREVIEW_MAX_CHARACTERS),
            url: message.url,
            wants_attention: message.unread,
        })
        .collect();
    Ok(Reading {
        sign_in: SignInState::SignedIn,
        items,
    })
}

fn truncate(text: &str, max_characters: usize) -> String {
    text.trim().chars().take(max_characters).collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_page_reading_keeps_its_messages_in_order() {
        let reading = parse_page(&json!({
            "signedIn": true,
            "messages": [
                { "id": "AAQk1", "from": "Ada <ada@example.com>", "subject": "Launch", "preview": "Ready?", "unread": true },
                { "id": "AAQk2", "from": "Grace", "subject": "Notes", "preview": "", "unread": false,
                  "receivedAt": "2026-10-20T09:15:00Z" },
            ],
        }))
        .expect("a reading");

        assert_eq!(reading.sign_in, SignInState::SignedIn);
        assert_eq!(reading.items.len(), 2);
        assert_eq!(reading.items[0].external_id, "AAQk1");
        assert!(reading.items[0].wants_attention);
        assert!(!reading.items[1].wants_attention);
        assert_eq!(
            reading.items[1].occurred_at.map(|at| at.to_rfc3339()),
            Some("2026-10-20T09:15:00+00:00".to_owned())
        );
    }

    #[test]
    fn a_date_that_does_not_parse_is_absent_not_an_error() {
        let reading = parse_page(&json!({
            "signedIn": true,
            "messages": [
                { "id": "a", "receivedAt": "Tue, 20 Oct 2026 09:15:00 GMT" },
                { "id": "b", "receivedAt": "2026-10-20T09:15:00-04:00" },
            ],
        }))
        .expect("a reading");
        assert_eq!(reading.items[0].occurred_at, None);
        assert_eq!(
            reading.items[1].occurred_at.map(|at| at.to_rfc3339()),
            Some("2026-10-20T13:15:00+00:00".to_owned())
        );
    }

    #[test]
    fn gmail_reads_its_mailbox_and_knows_its_sign_in_page() {
        let gmail = service(ConnectorKind::Gmail).expect("Gmail is a webmail service");
        assert!(starts_with_any(
            "https://mail.google.com/mail/u/0/#inbox",
            gmail.mailbox_origins
        ));
        assert!(starts_with_any(
            "https://accounts.google.com/v3/signin/identifier",
            gmail.sign_in_origins
        ));
        assert!(!starts_with_any(
            "https://outlook.office.com/mail/",
            gmail.mailbox_origins
        ));
    }

    #[test]
    fn a_signed_out_page_reads_nothing() {
        let reading = parse_page(&json!({ "signedIn": false, "messages": [] })).expect("a reading");
        assert_eq!(reading.sign_in, SignInState::SignedOut);
        assert!(reading.items.is_empty());
    }

    #[test]
    fn messages_without_an_id_are_dropped_and_long_text_is_cut() {
        let reading = parse_page(&json!({
            "signedIn": true,
            "messages": [
                { "id": " ", "subject": "no id" },
                { "id": "x", "preview": "p".repeat(5000) },
            ],
        }))
        .expect("a reading");
        assert_eq!(reading.items.len(), 1);
        assert_eq!(
            reading.items[0].body.chars().count(),
            PREVIEW_MAX_CHARACTERS
        );
    }

    #[test]
    fn an_answer_of_the_wrong_shape_is_an_error() {
        assert!(matches!(
            parse_page(&json!("not a page")),
            Err(ConnectorError::Unexpected(_))
        ));
    }

    #[test]
    fn every_service_is_found_by_its_kind_and_stored_as_text_it_parses() {
        for row in &SERVICES {
            assert_eq!(service(row.kind).map(|found| found.kind), Some(row.kind));
            assert_eq!(ConnectorKind::parse(row.kind.as_str()), Some(row.kind));
        }
        for kind in ConnectorKind::WEBMAIL {
            assert!(service(kind).is_some(), "{kind:?} has a webmail service");
        }
        for kind in ConnectorKind::MEETINGS {
            assert!(service(kind).is_none(), "{kind:?} reads meetings, not mail");
        }
    }
}
