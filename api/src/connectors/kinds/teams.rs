// Copyright © 2026 Jalapeno Labs

//! Microsoft Teams: a person's chats and channels, and their recorded meetings, read through a
//! browser they signed in to. See `docs/connectors.md`, `docs/meetings.md`,
//! `crate::connectors::teams`, and `crate::connectors::teams::meetings`.
//!
//! The account is the connector's own browser, started with it at Teams' start page
//! (`super::browser`); its person signs in to Microsoft on the stream the connector's page
//! shows. The reader opens Teams in a tab of its own when the browser has none. Nothing is
//! installed into the organization, and removing the connector removes the browser.
//!
//! # What it reads
//!
//! Its content picks the conversations the chat reader records (`conversations`), whether it
//! reads the person's recorded meetings too (`readMeetings`, on until turned off), and whether a
//! meeting waits for its person's approval before it is acted on (`waitForApproval`, as every
//! meeting source asks). A meeting's recording is kept in the connector's storage location, a
//! column of its own, and meetings are read only once one is picked: a browser connector starts
//! before its person can choose one, and a meeting is fetched once, so reading earlier would lose
//! those recordings for good.

use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::browser::{browser_of, owned_browser, starts_with_its_browser};
use super::fireflies::ensure_location;
use super::{
    AccountChoice, ContentField, ContentFieldType, Definition, Provides, Replies, Source,
    parse_content,
};
use crate::auth::CurrentUser;
use crate::browser::{BrowserState, Browsers};
use crate::connectors::checks::{CheckSpec, Checks, Outcome};
use crate::connectors::teams::meetings::TeamsMeetings;
use crate::connectors::teams::{self as reader, START_URL, Selection, Teams, TeamsReading};
use crate::connectors::{
    AccountKind, ConnectorError, ConnectorKind, Outgoing, Scope, Sent, SignInState,
};
use crate::errors::ApiError;
use crate::meetings::WAIT_FOR_APPROVAL_FIELD;
use crate::meetings::source::{ListedMeeting, MeetingSource, SourceError};
use crate::models::browser_session::BrowserSession;
use crate::models::connector::{self, Connector};
use crate::models::draft::ReplyPlace;
use crate::models::timeline_record::TimelineRecord;
use crate::state::AppState;

/// The content field that turns meetings on or off.
const READ_MEETINGS_FIELD: &str = "readMeetings";

/// How far back the meetings check looks for a recording to sample, as far as a new
/// connector's first listing reaches.
const SAMPLE_DAYS: i64 = 14;

/// How many recordings the meetings check lists.
const SAMPLE_SIZE: u32 = 5;

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
    CheckSpec {
        id: "list_meetings",
        required: false,
        depends_on: &["signed_in"],
    },
    CheckSpec {
        id: "sample_transcript",
        required: false,
        depends_on: &["list_meetings"],
    },
];

pub const DEFINITION: Definition = Definition {
    kind: ConnectorKind::Teams,
    scope: Scope::Personal,
    account: AccountKind::Browser,
    provides: &[Provides::Messages, Provides::Meetings],
    start_url: Some(START_URL),
    content: &[
        ContentField {
            id: "conversations",
            field: ContentFieldType::Selection,
            required: true,
        },
        ContentField {
            id: READ_MEETINGS_FIELD,
            field: ContentFieldType::Toggle {
                on_by_default: true,
            },
            required: false,
        },
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
    checks: CHECKS,
    replies: Replies::Connector {
        places: &[ReplyPlace::Thread, ReplyPlace::Conversation],
    },
};

/// What a Teams connector reads, as its form sends it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Content {
    /// `"*"` or a list of conversation ids, read by [`Selection::from_content`].
    conversations: Value,
    /// On unless its person turns it off; a form saved before the field existed sends none.
    #[serde(default = "meetings_on_by_default")]
    read_meetings: bool,
    #[serde(default)]
    wait_for_approval: bool,
    #[serde(default)]
    storage_location_id: Option<Uuid>,
}

const fn meetings_on_by_default() -> bool {
    true
}

impl Content {
    /// The content `value` holds, with its conversations checked.
    fn parse(value: Value) -> Result<(Self, Selection), ApiError> {
        let content: Self = parse_content(ConnectorKind::Teams, value)?;
        let selection = selection(&json!({ "conversations": content.conversations }))?;
        Ok((content, selection))
    }

    /// What the connector keeps in `connectors.content`. The storage location is a column of
    /// its own, so a removed location is caught by its foreign key.
    fn stored(&self, selection: &Selection) -> Value {
        let conversations = match selection {
            Selection::All => json!("*"),
            Selection::Only(ids) => {
                let mut ids: Vec<&String> = ids.iter().collect();
                ids.sort();
                json!(ids)
            }
        };
        json!({
            "conversations": conversations,
            READ_MEETINGS_FIELD: self.read_meetings,
            WAIT_FOR_APPROVAL_FIELD: self.wait_for_approval,
        })
    }
}

/// Whether a Teams connector with `content`, keeping files in `storage_location_id`, reads its
/// person's meetings: while they leave them on, and once it has somewhere to keep recordings.
pub fn reads_meetings(content: &Value, storage_location_id: Option<Uuid>) -> bool {
    let turned_on = content
        .get(READ_MEETINGS_FIELD)
        .and_then(Value::as_bool)
        .unwrap_or_else(meetings_on_by_default);
    turned_on && storage_location_id.is_some()
}

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
        meetings: state.meeting_sources.teams().clone(),
        browsers: state.browsers.clone(),
        browser: browser_of(state, stored).await?,
        selection: selection(&stored.content)?,
        meetings_turned_on: stored
            .content
            .get(READ_MEETINGS_FIELD)
            .and_then(Value::as_bool)
            .unwrap_or_else(meetings_on_by_default),
        has_storage_location: stored.storage_location_id.is_some(),
        reading: None,
        recordings: Vec::new(),
    })
}

/// Teams' checks against one browser.
pub struct TeamsChecks {
    reader: Teams,
    meetings: TeamsMeetings,
    browsers: Browsers,
    browser: BrowserSession,
    selection: Selection,
    meetings_turned_on: bool,
    has_storage_location: bool,
    /// One read of everything Teams has stored, shared by every check that needs it.
    reading: Option<Result<TeamsReading, String>>,
    /// The recordings `list_meetings` found, newest first, for `sample_transcript`.
    recordings: Vec<ListedMeeting>,
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
            "list_meetings" => self.list_meetings().await,
            "sample_transcript" => self.sample_transcript().await,
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

    /// The recorded meetings of the last two weeks, as the first listing would find them.
    async fn list_meetings(&mut self) -> Outcome {
        if !self.meetings_turned_on {
            return Outcome::skipped("meetings_off");
        }
        let since = chrono::Utc::now() - chrono::Duration::days(SAMPLE_DAYS);
        let listed = self
            .meetings
            .list(&self.browser, Some(since), SAMPLE_SIZE)
            .await;
        match listed {
            Ok(recordings) => {
                self.recordings = recordings;
                let counted = Outcome::counted(self.recordings.len());
                if self.has_storage_location {
                    return counted;
                }
                // Readable, but not read until a storage location is picked.
                Outcome {
                    reason: Some("no_storage_location"),
                    ..counted
                }
            }
            Err(error) => Outcome::failed(error.to_string()),
        }
    }

    /// The newest recording's transcript, and whether its recording can be downloaded.
    async fn sample_transcript(&mut self) -> Outcome {
        let Some(newest) = self.recordings.first() else {
            return Outcome::skipped("no_meetings");
        };
        match self
            .meetings
            .fetch(&self.browser, &newest.external_id)
            .await
        {
            Ok(fetched) => Outcome {
                message: Some(newest.title.clone()),
                reason: fetched
                    .recording
                    .is_none()
                    .then_some("recording_not_downloadable"),
                ..Outcome::counted(fetched.transcript.len())
            },
            Err(SourceError::NotFound) => Outcome::skipped("meeting_unavailable"),
            Err(error) => Outcome::failed(error.to_string()),
        }
    }
}

/// Connects Teams for `current`, reading the conversations `content` names, or every one, with
/// a browser of its own to sign in on.
///
/// # Errors
/// [`ApiError::BadRequest`] for content that is not a Teams connector's, or that names no
/// storage location, and as `super::browser::create`.
pub async fn create(
    state: &AppState,
    current: &CurrentUser,
    content: Value,
) -> Result<Connector, ApiError> {
    let (content, selection) = Content::parse(content)?;
    if let Some(storage_location_id) = content.storage_location_id {
        ensure_location(state, storage_location_id).await?;
    }
    super::browser::create(
        state,
        current,
        ConnectorKind::Teams,
        content.stored(&selection),
        content.storage_location_id,
    )
    .await
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

/// Replaces what a Teams connector reads, and where it keeps new recordings. Meetings already
/// kept stay where they are, and a meeting already corrected keeps the decision its correction
/// made.
///
/// # Errors
/// [`ApiError::BadRequest`] for content that is not a Teams connector's, or that names no
/// storage location.
pub async fn update_content(
    state: &AppState,
    stored: &Connector,
    content: Value,
) -> Result<Connector, ApiError> {
    let (content, selection) = Content::parse(content)?;
    if let Some(storage_location_id) = content.storage_location_id {
        ensure_location(state, storage_location_id).await?;
    }
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let updated = connector::set_content(
        &mut connection,
        stored.id,
        content.stored(&selection),
        content.storage_location_id,
    )
    .await?;
    state.connectors.activity.changed(stored.id);
    Ok(updated)
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
    fn content_is_stored_with_its_conversations_sorted_and_meetings_on_by_default() {
        let (content, selection) =
            Content::parse(json!({ "conversations": ["19:b", "19:a"] })).expect("content");
        assert_eq!(
            content.stored(&selection),
            json!({
                "conversations": ["19:a", "19:b"],
                "readMeetings": true,
                "waitForApproval": false,
            })
        );
        let (everything, all) = Content::parse(json!({
            "conversations": "*",
            "readMeetings": false,
            "waitForApproval": true,
            "storageLocationId": null,
        }))
        .expect("content");
        assert_eq!(all, Selection::All);
        assert_eq!(
            everything.stored(&all),
            json!({ "conversations": "*", "readMeetings": false, "waitForApproval": true })
        );
        assert!(matches!(
            Selection::from_content(&json!({ "conversations": ["19:a"] })),
            Ok(Selection::Only(ids)) if ids == HashSet::from(["19:a".to_owned()])
        ));
    }

    #[test]
    fn content_that_is_not_a_teams_connectors_is_refused() {
        for refused in [
            json!({ "conversations": 7 }),
            json!({ "conversations": "*", "readMeetings": "yes" }),
            json!({ "conversations": "*", "spaces": "*" }),
        ] {
            assert!(
                matches!(
                    Content::parse(refused.clone()),
                    Err(ApiError::BadRequest(_))
                ),
                "{refused}"
            );
        }
    }

    #[test]
    fn meetings_are_read_while_on_and_once_recordings_have_somewhere_to_go() {
        let location = Some(Uuid::now_v7());
        assert!(reads_meetings(&json!({ "conversations": "*" }), location));
        assert!(reads_meetings(&json!({ "readMeetings": true }), location));
        assert!(!reads_meetings(&json!({ "readMeetings": false }), location));
        assert!(!reads_meetings(&json!({ "readMeetings": true }), None));
    }
}
