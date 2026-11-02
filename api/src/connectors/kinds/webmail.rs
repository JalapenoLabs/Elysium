// Copyright © 2026 Jalapeno Labs

//! Outlook and Gmail: a person's inbox, read through a browser they signed in to. See
//! `docs/connectors.md` and `docs/browser.md`.
//!
//! The account is the browser. A new one is made and signed in to on its own page under
//! Settings, Browser sign-ins, which streams it; the hub then connects the mailbox through it.
//! A browser feeds each kind at most once, and removing the connector leaves the browser, and
//! its sign-in, as it was.

use serde_json::Value;
use uuid::Uuid;

use super::{AccountChoice, AccountDefinition, Definition, Source};
use crate::auth::CurrentUser;
use crate::browser::{BrowserState, Browsers};
use crate::connectors::checks::{CheckSpec, Checks, Outcome};
use crate::connectors::webmail::{Webmail, service};
use crate::connectors::{AccountKind, BrowserReader, ConnectorKind, Reading, Scope, SignInState};
use crate::errors::ApiError;
use crate::models::browser_session::{self, BrowserSession};
use crate::models::connector::{self, AccountRef, Connector, NewConnector};
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
    start_url: Some("https://outlook.office.com/mail/"),
    content: &[],
    checks: CHECKS,
};

pub const GMAIL: Definition = Definition {
    kind: ConnectorKind::Gmail,
    scope: Scope::Personal,
    account: AccountKind::Browser,
    start_url: Some("https://mail.google.com/mail/"),
    content: &[],
    checks: CHECKS,
};

pub const ACCOUNT: AccountDefinition = AccountDefinition {
    kind: AccountKind::Browser,
    fields: &[],
};

/// The browser `id`, when `owner` created it.
async fn owned_browser(
    state: &AppState,
    id: Uuid,
    owner: Uuid,
) -> Result<BrowserSession, ApiError> {
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let browser = browser_session::find(&mut connection, id)
        .await?
        .filter(|browser| browser.created_by == owner)
        .ok_or(ApiError::NotFound)?;
    Ok(browser)
}

pub async fn prepare_checks(
    state: &AppState,
    current: &CurrentUser,
    kind: ConnectorKind,
    source: Source<'_>,
) -> Result<WebmailChecks, ApiError> {
    let browser_id = match source {
        Source::Stored(stored) => stored.browser_session_id.ok_or_else(|| {
            ApiError::BadRequest("a mailbox connector reads through a browser".to_owned())
        })?,
        Source::Draft { account, .. } => match account {
            AccountChoice::Existing { id } => id,
            AccountChoice::New { .. } => return Err(sign_in_first()),
        },
    };
    let browser = owned_browser(state, browser_id, current.id()).await?;
    let service = service(kind)
        .ok_or_else(|| ApiError::BadRequest(format!("{} is not a mailbox", kind.as_str())))?;
    Ok(WebmailChecks {
        reader: Webmail::new(service, state.connectors.ingest.devtools().clone()),
        browsers: state.browsers.clone(),
        browser,
        reading: None,
    })
}

fn sign_in_first() -> ApiError {
    ApiError::BadRequest(
        "a mailbox is read through a browser: sign in to one under Settings, Browser sign-ins first"
            .to_owned(),
    )
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
                let status = self.browsers.status(self.browser.id);
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
            let read = self
                .reader
                .read(&self.browser)
                .await
                .map_err(|error| error.to_string());
            self.reading = Some(read);
        }
        self.reading
            .clone()
            .unwrap_or_else(|| Err("the mailbox was not read".to_owned()))
    }
}

/// Connects the mailbox `kind` reads through one of `current`'s browsers.
///
/// # Errors
/// [`ApiError::BadRequest`] for a new account (a browser is signed in to on its own page) or any
/// content, [`ApiError::NotFound`] for a browser `current` does not have, and
/// [`ApiError::Conflict`] when the browser already feeds this kind.
pub async fn create(
    state: &AppState,
    current: &CurrentUser,
    kind: ConnectorKind,
    account: AccountChoice,
    content: Value,
) -> Result<Connector, ApiError> {
    let AccountChoice::Existing { id } = account else {
        return Err(sign_in_first());
    };
    if content.as_object().is_some_and(|fields| !fields.is_empty()) {
        return Err(ApiError::BadRequest(format!(
            "{} reads the whole inbox and takes no content",
            kind.as_str()
        )));
    }
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let created = connector::create(
        &mut connection,
        NewConnector::new(kind.as_str(), AccountRef::Browser(id), current.id()),
    )
    .await
    .map_err(|error| match error {
        diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        ) => ApiError::Conflict("this browser already reads that mailbox"),
        other => other.into(),
    })?;
    Ok(created)
}
