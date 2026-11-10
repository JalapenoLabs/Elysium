// Copyright © 2026 Jalapeno Labs

//! Fireflies through a browser: a person's meetings, read through Fireflies' own web app in a
//! browser they signed in to, on any plan. See `docs/meetings.md` and
//! `crate::meetings::fireflies_browser`.
//!
//! Fireflies' API key reads transcripts and recordings only on its paid plans, so this kind
//! stands beside the key kind (`super::fireflies`) for everyone else. The account is the
//! connector's own browser, started with it at Fireflies' sign-in page (`super::browser`); its
//! person signs in on the stream the connector's page shows, and removing the connector removes
//! the browser. Its content is where recordings are kept, chosen once the browser is up, and
//! whether to wait for its person's approval before acting on meetings.

use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::browser::{browser_of, starts_with_its_browser};
use super::{ContentField, ContentFieldType, Definition, Provides, Replies, Source, parse_content};
use crate::auth::CurrentUser;
use crate::browser::{BrowserState, Browsers};
use crate::connectors::checks::{CheckSpec, Checks, Outcome};
use crate::connectors::{AccountKind, ConnectorKind, Scope, SignInState};
use crate::errors::ApiError;
use crate::meetings::WAIT_FOR_APPROVAL_FIELD;
use crate::meetings::fireflies_browser::{FirefliesBrowser, START_URL};
use crate::meetings::source::{ListedMeeting, MeetingSource, SourceError};
use crate::models::browser_session::BrowserSession;
use crate::models::connector::{self, Connector};
use crate::models::storage_location;
use crate::state::AppState;

/// How far back a check looks for a meeting to sample, as far as a new connector's first
/// listing reaches (`crate::meetings`).
const SAMPLE_DAYS: i64 = 14;

/// How many meetings the listing check asks for: the most one listing holds, so its count is
/// the two weeks' meetings rather than the oldest few of them.
const SAMPLE_SIZE: u32 = 150;

pub const DEFINITION: Definition = Definition {
    kind: ConnectorKind::FirefliesBrowser,
    scope: Scope::Personal,
    account: AccountKind::Browser,
    provides: &[Provides::Meetings],
    start_url: Some(START_URL),
    content: &[
        // Not required: the connector is made with its browser before anything is filled in,
        // and a meeting with nowhere to keep its recording goes on from its transcript alone.
        ContentField {
            id: "storageLocationId",
            field: ContentFieldType::StorageLocation,
            required: false,
        },
        ContentField {
            id: WAIT_FOR_APPROVAL_FIELD,
            field: ContentFieldType::Toggle {
                on_by_default: false,
            },
            required: false,
        },
    ],
    checks: &[
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
            id: "list_meetings",
            required: true,
            depends_on: &["signed_in"],
        },
        CheckSpec {
            id: "sample_transcript",
            required: false,
            depends_on: &["list_meetings"],
        },
    ],
    replies: Replies::Never,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Content {
    #[serde(default)]
    storage_location_id: Option<Uuid>,
    /// Off unless its person turns it on.
    #[serde(default)]
    wait_for_approval: bool,
}

impl Content {
    /// What the connector keeps in `connectors.content`. The storage location is a column of
    /// its own, so a removed location is caught by its foreign key.
    fn stored(&self) -> Value {
        json!({ WAIT_FOR_APPROVAL_FIELD: self.wait_for_approval })
    }
}

pub async fn prepare_checks(
    state: &AppState,
    source: Source<'_>,
) -> Result<FirefliesBrowserChecks, ApiError> {
    let Source::Stored(stored) = source else {
        return Err(starts_with_its_browser());
    };
    Ok(FirefliesBrowserChecks {
        reader: state.meeting_sources.fireflies_browser().clone(),
        browsers: state.browsers.clone(),
        browser: browser_of(state, stored).await?,
        meetings: Vec::new(),
    })
}

/// The browser kind's checks against one browser.
pub struct FirefliesBrowserChecks {
    reader: FirefliesBrowser,
    browsers: Browsers,
    browser: BrowserSession,
    meetings: Vec<ListedMeeting>,
}

impl std::fmt::Debug for FirefliesBrowserChecks {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FirefliesBrowserChecks")
            .field("browser", &self.browser.id)
            .finish_non_exhaustive()
    }
}

impl Checks for FirefliesBrowserChecks {
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
            "signed_in" => match self.reader.sign_in_state(&self.browser).await {
                Ok(SignInState::SignedIn) => Outcome::passed(),
                Ok(SignInState::SignedOut) => Outcome::failed_because("signed_out"),
                Ok(SignInState::Unknown) => Outcome::failed_because("fireflies_not_open"),
                Err(error) => Outcome::failed(error.to_string()),
            },
            "list_meetings" => {
                let since = chrono::Utc::now() - chrono::Duration::days(SAMPLE_DAYS);
                match self
                    .reader
                    .list(&self.browser, Some(since), SAMPLE_SIZE)
                    .await
                {
                    Ok(meetings) => {
                        self.meetings = meetings;
                        Outcome::counted(self.meetings.len())
                    }
                    Err(error) => Outcome::failed(error.to_string()),
                }
            }
            "sample_transcript" => {
                let Some(meeting) = self.meetings.first() else {
                    return Outcome::skipped("no_meetings");
                };
                match self.reader.fetch(&self.browser, &meeting.external_id).await {
                    Ok(fetched) => Outcome {
                        message: Some(meeting.title.clone()),
                        ..Outcome::counted(fetched.transcript.len())
                    },
                    Err(SourceError::NotFound) => Outcome::skipped("meeting_unavailable"),
                    Err(error) => Outcome::failed(error.to_string()),
                }
            }
            unknown => Outcome::failed(format!("Fireflies (browser) has no check {unknown}")),
        }
    }
}

/// Checks `storage_location_id`, when there is one, names a storage location.
async fn ensure_location(
    state: &AppState,
    storage_location_id: Option<Uuid>,
) -> Result<(), ApiError> {
    let Some(storage_location_id) = storage_location_id else {
        return Ok(());
    };
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    match storage_location::find(&mut connection, storage_location_id).await {
        Ok(_location) => Ok(()),
        Err(diesel::result::Error::NotFound) => Err(ApiError::BadRequest(
            "storageLocationId names no storage location".to_owned(),
        )),
        Err(other) => Err(other.into()),
    }
}

/// Connects `current`'s meetings through a browser of its own to sign in to Fireflies on,
/// keeping recordings in the content's storage location when it names one.
///
/// # Errors
/// [`ApiError::BadRequest`] for content that is not this kind's or names no storage location,
/// and as `super::browser::create`.
pub async fn create(
    state: &AppState,
    current: &CurrentUser,
    content: Value,
) -> Result<Connector, ApiError> {
    let content: Content = parse_content(ConnectorKind::FirefliesBrowser, content)?;
    ensure_location(state, content.storage_location_id).await?;
    super::browser::create(
        state,
        current,
        ConnectorKind::FirefliesBrowser,
        content.stored(),
        content.storage_location_id,
    )
    .await
}

/// Sets where the connector keeps new recordings, or that it keeps none, and whether it waits
/// for its person's approval before acting on meetings. Meetings already kept stay where they
/// are.
///
/// # Errors
/// [`ApiError::BadRequest`] for content that names no storage location.
pub async fn update_content(
    state: &AppState,
    stored: &Connector,
    content: Value,
) -> Result<Connector, ApiError> {
    let content: Content = parse_content(ConnectorKind::FirefliesBrowser, content)?;
    ensure_location(state, content.storage_location_id).await?;
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let updated = connector::set_content(
        &mut connection,
        stored.id,
        content.stored(),
        content.storage_location_id,
    )
    .await?;
    state.connectors.activity.changed(stored.id);
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_connector_needs_no_content_and_keeps_only_the_toggle() {
        let empty: Content = parse_content(ConnectorKind::FirefliesBrowser, json!({}))
            .expect("a new browser connector is made with nothing filled in");
        assert_eq!(empty.storage_location_id, None);
        assert_eq!(empty.stored(), json!({ "waitForApproval": false }));

        let location = Uuid::now_v7();
        let filled: Content = parse_content(
            ConnectorKind::FirefliesBrowser,
            json!({ "storageLocationId": location, "waitForApproval": true }),
        )
        .expect("parses");
        assert_eq!(filled.storage_location_id, Some(location));
        assert_eq!(filled.stored(), json!({ "waitForApproval": true }));

        let cleared: Content = parse_content(
            ConnectorKind::FirefliesBrowser,
            json!({ "storageLocationId": null, "waitForApproval": false }),
        )
        .expect("a cleared location parses");
        assert_eq!(cleared.storage_location_id, None);
    }

    #[test]
    fn content_of_another_kind_is_refused() {
        assert!(matches!(
            parse_content::<Content>(
                ConnectorKind::FirefliesBrowser,
                json!({ "conversations": "*" })
            ),
            Err(ApiError::BadRequest(_))
        ));
    }
}
