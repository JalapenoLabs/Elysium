// Copyright © 2026 Jalapeno Labs

//! Microsoft Teams: a person's chats and channels, read through a browser they signed in to.
//! See `docs/connectors.md` and `crate::connectors::teams`.
//!
//! The account is the browser, as for Outlook: a browser signed in to Microsoft for Outlook
//! reads Teams too, opening Teams in a tab of its own when it has none. Nothing is installed
//! into the organization. A browser feeds Teams at most once, and removing the connector leaves
//! the browser, and its sign-in, as it was.

use serde_json::{Value, json};

use super::webmail::{owned_browser, sign_in_first};
use super::{AccountChoice, AccountChoices, ContentField, ContentFieldType, Definition, Source};
use crate::auth::CurrentUser;
use crate::browser::{BrowserState, Browsers};
use crate::connectors::checks::{CheckSpec, Checks, Outcome};
use crate::connectors::teams::{START_URL, Selection, Teams, TeamsReading};
use crate::connectors::{AccountKind, ConnectorKind, Scope, SignInState};
use crate::errors::ApiError;
use crate::models::browser_session::BrowserSession;
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
        id: "chats_readable",
        required: true,
        depends_on: &["signed_in"],
    },
    CheckSpec {
        id: "sample_message",
        required: false,
        depends_on: &["chats_readable"],
    },
];

pub const DEFINITION: Definition = Definition {
    kind: ConnectorKind::Teams,
    scope: Scope::Personal,
    account: AccountKind::Browser,
    accounts: AccountChoices::Existing,
    start_url: Some(START_URL),
    content: &[ContentField {
        id: "conversations",
        field: ContentFieldType::Selection,
        required: true,
    }],
    checks: CHECKS,
};

/// The conversations a draft or stored connector reads.
fn selection(content: &Value) -> Result<Selection, ApiError> {
    Selection::from_content(content)
        .map_err(|reason| ApiError::BadRequest(format!("teams content is not valid: {reason}")))
}

/// The browser a Teams connector reads through, stored or being set up.
async fn browser_of(
    state: &AppState,
    current: &CurrentUser,
    account: Option<&AccountChoice>,
    stored: Option<&Connector>,
) -> Result<BrowserSession, ApiError> {
    let id = match (stored, account) {
        (Some(stored), _) => stored.browser_session_id.ok_or_else(|| {
            ApiError::BadRequest("a Teams connector reads through a browser".to_owned())
        })?,
        (None, Some(AccountChoice::Existing { id })) => *id,
        (None, _) => return Err(sign_in_first()),
    };
    owned_browser(state, id, current.id()).await
}

pub async fn prepare_checks(
    state: &AppState,
    current: &CurrentUser,
    source: Source<'_>,
) -> Result<TeamsChecks, ApiError> {
    let (browser, selection) = match source {
        Source::Stored(stored) => (
            browser_of(state, current, None, Some(stored)).await?,
            selection(&stored.content)?,
        ),
        Source::Draft { account, content } => (
            browser_of(state, current, Some(&account), None).await?,
            selection(&content)?,
        ),
    };
    Ok(TeamsChecks {
        reader: Teams::new(state.connectors.ingest.devtools().clone()),
        browsers: state.browsers.clone(),
        browser,
        selection,
        reading: None,
    })
}

/// Teams' checks against one browser.
pub struct TeamsChecks {
    reader: Teams,
    browsers: Browsers,
    browser: BrowserSession,
    selection: Selection,
    /// One read of everything Teams has stored, shared by every check that needs it.
    reading: Option<Result<TeamsReading, String>>,
}

impl std::fmt::Debug for TeamsChecks {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TeamsChecks")
            .field("browser", &self.browser.id)
            .finish_non_exhaustive()
    }
}

impl Checks for TeamsChecks {
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
                    SignInState::Unknown => Outcome::failed_because("teams_not_open"),
                },
                Err(message) => Outcome::failed(message),
            },
            "chats_readable" => match self.read().await {
                Ok(reading) if reading.found => Outcome::counted(reading.conversations.len()),
                Ok(reading) => Outcome {
                    // What the page does hold, so the store's layout can be read off a check.
                    message: Some(format!(
                        "databases on the page: {}",
                        if reading.databases.is_empty() {
                            "none".to_owned()
                        } else {
                            reading.databases.join(", ")
                        }
                    )),
                    ..Outcome::failed_because("teams_store_not_found")
                },
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
            unknown => Outcome::failed(format!("Teams has no check {unknown}")),
        }
    }
}

impl TeamsChecks {
    /// Everything Teams has stored for the selected conversations, read once.
    async fn read(&mut self) -> Result<TeamsReading, String> {
        if self.reading.is_none() {
            let read = self
                .reader
                .read_teams(&self.browser, None, &self.selection)
                .await
                .map_err(|error| error.to_string());
            self.reading = Some(read);
        }
        self.reading
            .clone()
            .unwrap_or_else(|| Err("Teams was not read".to_owned()))
    }
}

/// Connects Teams through one of `current`'s browsers, reading the conversations `content`
/// names, or every one.
///
/// # Errors
/// [`ApiError::BadRequest`] for a new account (a browser is signed in to on its own page) or
/// content that is not a selection, [`ApiError::NotFound`] for a browser `current` does not
/// have, and [`ApiError::Conflict`] when the browser already feeds Teams.
pub async fn create(
    state: &AppState,
    current: &CurrentUser,
    account: AccountChoice,
    content: Value,
) -> Result<Connector, ApiError> {
    let AccountChoice::Existing { id } = account else {
        return Err(sign_in_first());
    };
    let content = stored_content(&selection(&content)?);
    owned_browser(state, id, current.id()).await?;
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let mut new = NewConnector::new(
        ConnectorKind::Teams.as_str(),
        AccountRef::Browser(id),
        current.id(),
    );
    new.content = content;
    let created = connector::create(&mut connection, new)
        .await
        .map_err(|error| match error {
            diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                _,
            ) => ApiError::Conflict("this browser already reads Teams"),
            other => other.into(),
        })?;
    Ok(created)
}

/// Every conversation the browser's Teams has stored, to pick from:
/// `{ conversations: { options: [{ id, label, detail }], truncated } }`.
///
/// # Errors
/// As [`create`], and [`ApiError::BadGateway`] when the browser cannot be read.
pub async fn options(
    state: &AppState,
    current: &CurrentUser,
    account: AccountChoice,
) -> Result<Value, ApiError> {
    let browser = browser_of(state, current, Some(&account), None).await?;
    let reader = Teams::new(state.connectors.ingest.devtools().clone());
    let reading = reader
        .read_teams(&browser, None, &Selection::All)
        .await
        .map_err(|error| ApiError::BadGateway(error.to_string()))?;
    let options: Vec<Value> = reading
        .conversations
        .iter()
        .map(|conversation| json!({ "id": conversation.id, "label": conversation.name, "detail": null }))
        .collect();
    Ok(json!({ "conversations": { "options": options, "truncated": false } }))
}

/// Replaces the conversations a Teams connector reads.
///
/// # Errors
/// [`ApiError::BadRequest`] for content that is not a selection.
pub async fn update_content(
    state: &AppState,
    stored: &Connector,
    content: Value,
) -> Result<Connector, ApiError> {
    let content = stored_content(&selection(&content)?);
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let updated = connector::set_content(
        &mut connection,
        stored.id,
        content,
        stored.storage_location_id,
    )
    .await?;
    Ok(updated)
}

/// A selection as `connectors.content` keeps it.
fn stored_content(selection: &Selection) -> Value {
    match selection {
        Selection::All => json!({ "conversations": "*" }),
        Selection::Only(ids) => {
            let mut ids: Vec<&String> = ids.iter().collect();
            ids.sort();
            json!({ "conversations": ids })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn a_selection_is_stored_as_everything_or_sorted_ids() {
        assert_eq!(
            stored_content(&Selection::All),
            json!({ "conversations": "*" })
        );
        let picked = Selection::Only(HashSet::from(["19:b".to_owned(), "19:a".to_owned()]));
        assert_eq!(
            stored_content(&picked),
            json!({ "conversations": ["19:a", "19:b"] })
        );
    }

    #[test]
    fn content_that_is_not_a_selection_is_refused() {
        assert!(matches!(
            selection(&json!({ "conversations": 7 })),
            Err(ApiError::BadRequest(_))
        ));
    }
}
