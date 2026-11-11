// Copyright © 2026 Jalapeno Labs

//! Outlook and Gmail: a person's inbox, read through a browser they signed in to. See
//! `docs/connectors.md` and `docs/browser.md`.
//!
//! The account is the connector's own browser, started with it at the mailbox's start page
//! (`super::browser`); its person signs in on the stream the connector's page shows. Removing
//! the connector removes the browser and its sign-in.

use serde_json::Value;

use super::browser::{browser_of, starts_with_its_browser};
use super::{AccountDefinition, Definition, Provides, Replies, Source};
use crate::auth::CurrentUser;
use crate::browser::{BrowserState, Browsers};
use crate::connectors::checks::{CheckSpec, Checks, Outcome};
use crate::connectors::webmail::{Webmail, service};
use crate::connectors::{
    AccountKind, BrowserReader, ConnectorKind, ReadInput, Reading, Scope, SignInState,
};
use crate::errors::ApiError;
use crate::models::browser_session::BrowserSession;
use crate::models::connector::Connector;
use crate::state::AppState;

const CHECKS: &[CheckSpec] = &[
    CheckSpec {
        id: "browser_running",
        required: true,
        depends_on: &[],
    },
    CheckSpec {
        id: "signed_in",
        required: true,
        depends_on: &["browser_running"],
    },
    CheckSpec {
        id: "inbox_readable",
        required: true,
        depends_on: &["signed_in"],
    },
    CheckSpec {
        id: "sample_message",
        required: false,
        depends_on: &["inbox_readable"],
    },
];

pub const OUTLOOK: Definition = Definition {
    kind: ConnectorKind::Outlook,
    scope: Scope::Personal,
    account: AccountKind::Browser,
    provides: &[Provides::Mail],
    start_url: Some("https://outlook.office.com/mail/"),
    content: &[],
    checks: CHECKS,
    replies: Replies::Mailbox,
};

pub const GMAIL: Definition = Definition {
    kind: ConnectorKind::Gmail,
    scope: Scope::Personal,
    account: AccountKind::Browser,
    provides: &[Provides::Mail],
    start_url: Some("https://mail.google.com/mail/"),
    content: &[],
    checks: CHECKS,
    replies: Replies::Mailbox,
};

pub const ACCOUNT: AccountDefinition = AccountDefinition {
    kind: AccountKind::Browser,
    fields: &[],
};

pub async fn prepare_checks(
    state: &AppState,
    kind: ConnectorKind,
    source: Source<'_>,
) -> Result<WebmailChecks, ApiError> {
    let Source::Stored(stored) = source else {
        return Err(starts_with_its_browser());
    };
    let browser = browser_of(state, stored).await?;
    let service = service(kind)
        .ok_or_else(|| ApiError::BadRequest(format!("{} is not a mailbox", kind.as_str())))?;
    Ok(WebmailChecks {
        reader: Webmail::new(service, state.connectors.ingest.devtools().clone()),
        browsers: state.browsers.clone(),
        browser,
        reading: None,
    })
}

/// A mailbox's checks against one browser.
pub struct WebmailChecks {
    reader: Webmail,
    browsers: Browsers,
    browser: BrowserSession,
    /// One read of the mailbox, shared by every check that needs it.
    reading: Option<Result<Reading, String>>,
}

impl std::fmt::Debug for WebmailChecks {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WebmailChecks")
            .field("browser", &self.browser.id)
            .finish_non_exhaustive()
    }
}

impl Checks for WebmailChecks {
    async fn check(&mut self, id: &'static str) -> Outcome {
        match id {
            "browser_running" => {
                let status = self.browsers.status(self.browser.id).await;
                if status.state == BrowserState::Running {
                    return Outcome::passed();
                }
                Outcome {
                    message: status.error,
                    ..Outcome::failed_because("browser_not_running")
                }
            }
            "signed_in" => match self.read().await {
                Ok(reading) => match reading.sign_in {
                    SignInState::SignedIn => Outcome::passed(),
                    SignInState::SignedOut => Outcome::failed_because("signed_out"),
                    SignInState::Unknown => Outcome::failed_because("mailbox_not_open"),
                },
                Err(message) => Outcome::failed(message),
            },
            "inbox_readable" => match self.read().await {
                Ok(reading) => Outcome::counted(reading.items.len()),
                Err(message) => Outcome::failed(message),
            },
            "sample_message" => match self.read().await {
                Ok(reading) if reading.items.is_empty() => Outcome::skipped("no_messages"),
                Ok(reading) => Outcome::counted(
                    reading
                        .items
                        .iter()
                        .filter(|item| item.wants_attention)
                        .count(),
                ),
                Err(message) => Outcome::failed(message),
            },
            unknown => Outcome::failed(format!("a mailbox has no check {unknown}")),
        }
    }
}

impl WebmailChecks {
    /// The mailbox as the browser shows it, read once.
    async fn read(&mut self) -> Result<Reading, String> {
        if self.reading.is_none() {
            // A mailbox keeps no state and reads no content.
            let input = ReadInput {
                state: &Value::Null,
                content: &Value::Null,
            };
            let read = self
                .reader
                .read(&self.browser, input)
                .await
                .map_err(|error| error.to_string());
            self.reading = Some(read);
        }
        self.reading
            .clone()
            .unwrap_or_else(|| Err("the mailbox was not read".to_owned()))
    }
}

/// Connects the mailbox `kind` reads for `current`, with a browser of its own to sign in on.
///
/// # Errors
/// [`ApiError::BadRequest`] for any content, and as `super::browser::create`.
pub async fn create(
    state: &AppState,
    current: &CurrentUser,
    kind: ConnectorKind,
    content: Value,
) -> Result<Connector, ApiError> {
    if content.as_object().is_some_and(|fields| !fields.is_empty()) {
        return Err(ApiError::BadRequest(format!(
            "{} reads the whole inbox and takes no content",
            kind.as_str()
        )));
    }
    super::browser::create(state, current, kind, content, None).await
}
