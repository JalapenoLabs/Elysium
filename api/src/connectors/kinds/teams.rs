// Copyright © 2026 Jalapeno Labs

//! Microsoft Teams: a person's chats and channels, read through a browser they signed in to.
//! See `docs/connectors.md` and `crate::connectors::teams`.
//!
//! The account is the connector's own browser, started with it at Teams' start page
//! (`super::browser`); its person signs in to Microsoft on the stream the connector's page
//! shows. The reader opens Teams in a tab of its own when the browser has none. Nothing is
//! installed into the organization, and removing the connector removes the browser.

use serde_json::{Value, json};

use super::browser::{browser_of, owned_browser, starts_with_its_browser};
use super::{AccountChoice, ContentField, ContentFieldType, Definition, Provides, Replies, Source};
use crate::auth::CurrentUser;
use crate::browser::{BrowserState, Browsers};
use crate::connectors::checks::{CheckSpec, Checks, Outcome};
use crate::connectors::teams::{self as reader, START_URL, Selection, Teams, TeamsReading};
use crate::connectors::{
    AccountKind, ConnectorError, ConnectorKind, Outgoing, Scope, Sent, SignInState,
};
use crate::errors::ApiError;
use crate::models::browser_session::BrowserSession;
use crate::models::connector::{self, Connector};
use crate::models::draft::ReplyPlace;
use crate::models::timeline_record::TimelineRecord;
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
    provides: &[Provides::Messages],
    start_url: Some(START_URL),
    content: &[ContentField {
        id: "conversations",
        field: ContentFieldType::Selection,
        required: true,
    }],
    checks: CHECKS,
    replies: Replies::Connector {
        places: &[ReplyPlace::Thread, ReplyPlace::Conversation],
    },
};

/// The conversations a draft or stored connector reads.
fn selection(content: &Value) -> Result<Selection, ApiError> {
    Selection::from_content(content)
        .map_err(|reason| ApiError::BadRequest(format!("teams content is not valid: {reason}")))
}

pub async fn prepare_checks(state: &AppState, source: Source<'_>) -> Result<TeamsChecks, ApiError> {
    let Source::Stored(stored) = source else {
        return Err(starts_with_its_browser());
    };
    Ok(TeamsChecks {
        reader: Teams::new(state.connectors.ingest.devtools().clone()),
        browsers: state.browsers.clone(),
        browser: browser_of(state, stored).await?,
        selection: selection(&stored.content)?,
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

/// Connects Teams for `current`, reading the conversations `content` names, or every one, with
/// a browser of its own to sign in on.
///
/// # Errors
/// [`ApiError::BadRequest`] for content that is not a selection, and as
/// `super::browser::create`.
pub async fn create(
    state: &AppState,
    current: &CurrentUser,
    content: Value,
) -> Result<Connector, ApiError> {
    let content = stored_content(&selection(&content)?);
    super::browser::create(state, current, ConnectorKind::Teams, content).await
}

/// Every conversation the browser's Teams has stored, to pick from:
/// `{ conversations: { options: [{ id, label, detail }], truncated } }`.
///
/// # Errors
/// [`ApiError::BadRequest`] for a new account, whose browser starts with its connector;
/// [`ApiError::NotFound`] for a browser `current` does not own; and [`ApiError::BadGateway`]
/// when the browser cannot be read.
pub async fn options(
    state: &AppState,
    current: &CurrentUser,
    account: AccountChoice,
) -> Result<Value, ApiError> {
    let AccountChoice::Existing { id } = account else {
        return Err(starts_with_its_browser());
    };
    let browser = owned_browser(state, id, current.id()).await?;
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

/// Where a reply to `record` goes, `place` in its conversation. See
/// `crate::connectors::teams::send`.
///
/// # Errors
/// [`ApiError::BadRequest`] for a record that does not say where its message was said.
pub fn reply_target(record: &TimelineRecord, place: ReplyPlace) -> Result<Value, ApiError> {
    let target = reader::send::reply_target(record, place).map_err(ApiError::BadRequest)?;
    serde_json::to_value(target).map_err(|error| ApiError::Internal(error.into()))
}

/// Posts `outgoing` as the person `connector`'s browser is signed in to Teams as.
///
/// # Errors
/// [`ConnectorError::Refused`] naming why it was not posted, including a connector still
/// waiting for its browser, and [`ConnectorError`] when the browser or the page fails.
pub async fn send(
    state: &AppState,
    connector: &Connector,
    outgoing: Outgoing<'_>,
) -> Result<Sent, ConnectorError> {
    let target: reader::send::Target = serde_json::from_value(outgoing.target.clone())
        .map_err(|error| ConnectorError::Unexpected(format!("a Teams draft's target: {error}")))?;
    let browser = browser_of(state, connector)
        .await
        .map_err(|error| ConnectorError::Refused(error.to_string()))?;
    Teams::new(state.connectors.ingest.devtools().clone())
        .post(&browser, &target, outgoing.body, outgoing.sender_name)
        .await
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
