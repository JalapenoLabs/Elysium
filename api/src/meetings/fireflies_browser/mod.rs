// Copyright © 2026 Jalapeno Labs

//! Fireflies.ai read through its own web app, in a browser its person signed in to.
//!
//! Fireflies' public API refuses transcripts and recordings below its paid plans, but its web
//! app reads every meeting, on any plan, from its own GraphQL endpoint (`/api/v4/hive` on
//! `app.fireflies.ai`), signed in by the session the person started in the connector's browser.
//! `hive.js` runs as that page, over `DevTools`, and asks the endpoint the queries below, so the
//! session never leaves the browser: only Fireflies' answers do. This module decides what they
//! mean. What Fireflies answers, and what broke before, is in `CLAUDE.md` beside this file.
//!
//! # What is read
//!
//! - A listing (`getFilteredMeetings`, the notebook's own query) of the meetings the person is
//!   part of, the web app's "My Meetings", that started since the connector's cursor, oldest
//!   first, so a listing that cannot hold them all leaves the rest for the next one.
//! - One meeting (`meetingNote`, the meeting page's own query) in two calls of one evaluation:
//!   its captions, each with a speaker and start and end times in seconds, with the edits its
//!   people saved in Fireflies applied as Fireflies shows them; then its video or audio address,
//!   apart, so a refusal of the recording never costs the transcript.
//!
//! # Meetings still being transcribed
//!
//! Fireflies lists a meeting as soon as it is recorded, before its transcript exists. A meeting
//! is fetched once, so one fetched early would be kept without its words. A listing therefore
//! stops at the first meeting still being transcribed ([`settled`]) and the cursor waits before
//! it; a meeting that has said it is transcribing for longer than [`TRANSCRIBING_PATIENCE`] is
//! taken as it is, so one stuck meeting never stops the rest.
//!
//! Nothing Fireflies' own AI wrote is asked for: no summary, action items, notes, outline, or
//! `AskFred`. The queries name what they read, field by field, and the key source's test holds
//! them to it (`crate::meetings::fireflies`).

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

use super::fireflies::{FetchedMedia, recording_of};
use super::source::{
    FetchedMeeting, ListedMeeting, MeetingSource, RecordingSource, SourceError, TranscriptLine,
};
use crate::browser::devtools::{Devtools, DevtoolsError};
use crate::connectors::{ConnectorError, SignInState};
use crate::models::browser_session::BrowserSession;

/// Where Fireflies' web app runs: a tab here is read.
pub const APP_ORIGIN: &str = "https://app.fireflies.ai/";

/// The page a new browser opens at, which sends a signed-in person on to their notebook.
pub const START_URL: &str = "https://app.fireflies.ai/login";

/// Where Fireflies sends its person to sign in with Google, Microsoft, or Apple.
pub const SIGN_IN_ORIGINS: &[&str] = &[
    "https://accounts.google.com/",
    "https://login.microsoftonline.com/",
    "https://login.live.com/",
    "https://appleid.apple.com/",
];

/// A meeting's page in the web app, which a record links to.
const MEETING_PAGE: &str = "https://app.fireflies.ai/view/";

const SCRIPT: &str = include_str!("hive.js");
const REQUEST_PLACEHOLDER: &str = "__ELYSIUM_REQUEST__";

/// The meetings the person is part of: the notebook's "My Meetings", which the web app maps to
/// this channel. `mine` is only the meetings they hosted.
const PART_OF_CHANNEL: &str = "USER_PART_OF";

/// The most meetings one listing asks for; the web app's own validation refuses more than 150.
const MAX_LISTING: u32 = 150;

/// How long a meeting may say it is being transcribed before a listing takes it as it is.
/// Fireflies transcribes a meeting within about its own length; a day is far past that, and
/// a meeting stuck longer would otherwise hold every later meeting back.
pub const TRANSCRIBING_PATIENCE: Duration = Duration::hours(24);

pub(super) const LIST_QUERY: &str = "query ElysiumMeetings(
  $from: Int!
  $size: Int!
  $channelId: String!
  $filters: SearchFilters
  $sort: [GlobalSearchSortInput!]
) {
  getFilteredMeetings(from: $from, size: $size, channelId: $channelId, filters: $filters, sort: $sort) {
    meetings { id title date durationMins creator_email allEmails processMeetingStatus }
  }
}";

/// A meeting's captions and the edits saved on them, which every plan reads.
pub(super) const TRANSCRIPT_QUERY: &str = "query ElysiumTranscript($meetingNoteId: String) {
  meetingNote(_id: $meetingNoteId) {
    processMeetingStatus
    speakerMeta
    sentenceMeta
    labelMeta
    captions { sentence speaker_id time endTime }
  }
}";

/// A meeting's recording addresses, asked for apart from its captions so that a refusal here
/// leaves the transcript read.
pub(super) const RECORDING_QUERY: &str = "query ElysiumRecording($meetingNoteId: String) {
  meetingNote(_id: $meetingNoteId) { video_url audio_url }
}";

/// Every query this source sends, for the test that none asks for what Fireflies' AI wrote.
#[cfg(test)]
pub(super) const QUERIES: [&str; 3] = [LIST_QUERY, TRANSCRIPT_QUERY, RECORDING_QUERY];

/// The error codes Fireflies' endpoint answers, from its web app's own handling.
const UNAUTHENTICATED: &str = "UNAUTHENTICATED";
const NOT_FOUND: &str = "NOT_FOUND_ERROR";

/// Reads Fireflies through a browser its person signed in to.
#[derive(Debug, Clone)]
pub struct FirefliesBrowser {
    devtools: Devtools,
}

/// One query for the page to ask.
#[derive(Debug, Serialize)]
struct Call<'query> {
    query: &'query str,
    variables: Value,
}

#[derive(Debug, Serialize)]
struct Request<'calls> {
    calls: &'calls [Call<'calls>],
}

/// What the page answered.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Answer {
    signed_in: bool,
    #[serde(default)]
    results: Vec<CallResult>,
}

/// What Fireflies answered one call: its HTTP status (0 when it was never sent), its data, and
/// its errors' messages and codes.
#[derive(Debug, Clone, Deserialize)]
struct CallResult {
    status: u16,
    #[serde(default)]
    data: Value,
    #[serde(default)]
    errors: Vec<HiveError>,
}

#[derive(Debug, Clone, Deserialize)]
struct HiveError {
    #[serde(default)]
    message: String,
    #[serde(default)]
    code: Option<String>,
}

/// What a tab showed when it was asked.
enum Tab {
    Open(Vec<CallResult>),
    /// No tab is on the web app, or its session ended.
    Missing(SignInState),
}

impl FirefliesBrowser {
    pub const fn new(devtools: Devtools) -> Self {
        Self { devtools }
    }

    /// Whether `browser` is signed in to Fireflies: the web app keeps its session in the page,
    /// so this asks nothing of Fireflies. With no Fireflies tab open, one is opened and this
    /// answers `Unknown`; the next ask reads it once it has loaded.
    ///
    /// # Errors
    /// [`ConnectorError`] when the browser does not answer or the script fails.
    pub async fn sign_in_state(
        &self,
        browser: &BrowserSession,
    ) -> Result<SignInState, ConnectorError> {
        match self.ask_page(browser, &[]).await? {
            Tab::Open(_results) => Ok(SignInState::SignedIn),
            Tab::Missing(state) => Ok(state),
        }
    }

    /// Asks the web app in `browser`'s tab `calls`, in order.
    async fn ask_page(
        &self,
        browser: &BrowserSession,
        calls: &[Call<'_>],
    ) -> Result<Tab, ConnectorError> {
        let port = u16::try_from(browser.port).map_err(|_overflow| {
            ConnectorError::Unexpected("a browser port out of range".to_owned())
        })?;
        let tabs = self.devtools.tabs(port).await?;
        if !tabs.iter().any(|tab| tab.url.starts_with(APP_ORIGIN)) {
            let signing_in = tabs.iter().any(|tab| {
                SIGN_IN_ORIGINS
                    .iter()
                    .any(|origin| tab.url.starts_with(origin))
            });
            if signing_in {
                return Ok(Tab::Missing(SignInState::SignedOut));
            }
            let start = Url::parse(START_URL).expect("Fireflies' start page parses");
            self.devtools.open_tab(port, &start).await?;
            return Ok(Tab::Missing(SignInState::Unknown));
        }

        let request = serde_json::to_string(&Request { calls })
            .map_err(|error| ConnectorError::Unexpected(error.to_string()))?;
        let script = SCRIPT.replace(REQUEST_PLACEHOLDER, &request);
        let value = match self.devtools.evaluate(port, &[APP_ORIGIN], &script).await {
            Ok(value) => value,
            Err(DevtoolsError::NoTab(_origins)) => return Ok(Tab::Missing(SignInState::Unknown)),
            Err(error) => return Err(error.into()),
        };
        let answer: Answer = serde_json::from_value(value)
            .map_err(|error| ConnectorError::Unexpected(error.to_string()))?;
        if !answer.signed_in {
            return Ok(Tab::Missing(SignInState::SignedOut));
        }
        Ok(Tab::Open(answer.results))
    }

    /// Asks `calls` and answers one result for each, as a meeting source's failure when the tab
    /// cannot answer at all.
    async fn ask(
        &self,
        browser: &BrowserSession,
        calls: &[Call<'_>],
    ) -> Result<Vec<CallResult>, SourceError> {
        let tab = self.ask_page(browser, calls).await.map_err(|error| {
            SourceError::Refused(format!("the browser could not be read: {error}"))
        })?;
        let results = match tab {
            Tab::Open(results) => results,
            Tab::Missing(SignInState::SignedOut) => return Err(signed_out()),
            Tab::Missing(SignInState::SignedIn | SignInState::Unknown) => {
                return Err(SourceError::Refused(
                    "Fireflies was not open in the browser; Elysium opened it, and the next \
                     read goes on once it has loaded"
                        .to_owned(),
                ));
            }
        };
        if results.len() != calls.len() {
            return Err(SourceError::Refused(format!(
                "the page answered {} of {} queries",
                results.len(),
                calls.len()
            )));
        }
        Ok(results)
    }
}

impl MeetingSource for FirefliesBrowser {
    /// The connector's own browser.
    type SignIn = BrowserSession;

    async fn list(
        &self,
        sign_in: &BrowserSession,
        since: Option<DateTime<Utc>>,
        limit: u32,
    ) -> Result<Vec<ListedMeeting>, SourceError> {
        let call = Call {
            query: LIST_QUERY,
            variables: listing_variables(since, limit),
        };
        let result = self
            .ask(sign_in, std::slice::from_ref(&call))
            .await?
            .remove(0);
        let data = data_of(result)?;
        let meetings: Vec<NoteMeeting> = serde_json::from_value(
            data["getFilteredMeetings"]["meetings"].clone(),
        )
        .map_err(|error| {
            SourceError::Refused(format!(
                "Fireflies listed meetings in an unknown shape: {error}"
            ))
        })?;
        Ok(settled(meetings, Utc::now()))
    }

    async fn fetch(
        &self,
        sign_in: &BrowserSession,
        external_id: &str,
    ) -> Result<FetchedMeeting, SourceError> {
        let variables = json!({ "meetingNoteId": external_id });
        let calls = [
            Call {
                query: TRANSCRIPT_QUERY,
                variables: variables.clone(),
            },
            Call {
                query: RECORDING_QUERY,
                variables,
            },
        ];
        let mut results = self.ask(sign_in, &calls).await?.into_iter();
        let (Some(transcript), Some(recording)) = (results.next(), results.next()) else {
            return Err(SourceError::Refused(
                "the page answered too few queries".to_owned(),
            ));
        };

        let note = data_of(transcript)?["meetingNote"].clone();
        if note.is_null() {
            return Err(SourceError::NotFound);
        }
        let note: MeetingNote = serde_json::from_value(note).map_err(|error| {
            SourceError::Refused(format!(
                "Fireflies sent a meeting in an unknown shape: {error}"
            ))
        })?;
        if !is_settled(note.process_meeting_status.as_deref()) {
            return Err(SourceError::Refused(
                "Fireflies is still transcribing the meeting".to_owned(),
            ));
        }
        Ok(FetchedMeeting {
            transcript: transcript_lines(note),
            recording: recording_from(recording, external_id),
        })
    }
}

/// The variables of a listing of at most `limit` meetings that started at or after `since`,
/// oldest first.
fn listing_variables(since: Option<DateTime<Utc>>, limit: u32) -> Value {
    let mut filters = json!({});
    if let Some(since) = since {
        filters["date"] =
            json!({ "from": since.to_rfc3339_opts(chrono::SecondsFormat::Millis, true) });
    }
    json!({
        "from": 0,
        "size": limit.clamp(1, MAX_LISTING),
        "channelId": PART_OF_CHANNEL,
        "filters": filters,
        "sort": [{ "field": "date", "order": "ASC" }],
    })
}

/// A call's data, or the source's failure for what Fireflies refused.
fn data_of(result: CallResult) -> Result<Value, SourceError> {
    if matches!(result.status, 401 | 403) {
        return Err(signed_out());
    }
    if let Some(error) = result.errors.first() {
        return Err(match error.code.as_deref() {
            Some(UNAUTHENTICATED) => signed_out(),
            Some(NOT_FOUND) => SourceError::NotFound,
            _ => SourceError::Refused(format!("Fireflies refused: {}", error.message)),
        });
    }
    if result.status != 200 {
        return Err(SourceError::Refused(format!(
            "Fireflies answered HTTP {}",
            result.status
        )));
    }
    Ok(result.data)
}

/// A meeting's video, else its audio, or `None` when Fireflies refused or offers neither. A
/// refusal is logged and costs only the recording.
fn recording_from(result: CallResult, external_id: &str) -> Option<RecordingSource> {
    let note = match data_of(result) {
        Ok(data) => data["meetingNote"].clone(),
        Err(error) => {
            tracing::event!(
                name: "meetings.fireflies_browser.recording.refused",
                tracing::Level::INFO,
                meeting.external_id = external_id,
                error.message = %error,
                "Fireflies offered no recording address, so the meeting goes on from its \
                 transcript alone",
            );
            return None;
        }
    };
    let media: Option<FetchedMedia> = serde_json::from_value(note).ok().flatten();
    media.and_then(recording_of)
}

fn signed_out() -> SourceError {
    SourceError::Unauthorized(
        "Fireflies is asking the connector's browser to sign in again; sign in on the \
         connector's page"
            .to_owned(),
    )
}

/// A listed meeting, as the web app's notebook query sends it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NoteMeeting {
    id: String,
    #[serde(default)]
    title: Option<String>,
    /// Milliseconds since the epoch, or an ISO 8601 time.
    #[serde(default)]
    date: Value,
    #[serde(default)]
    duration_mins: Option<f64>,
    #[serde(default, rename = "creator_email")]
    creator_email: Option<String>,
    /// A list of addresses, or one comma separated string of them.
    #[serde(default)]
    all_emails: Value,
    #[serde(default)]
    process_meeting_status: Option<String>,
}

/// Whether a meeting with `status` has its transcript, or never will: a status Fireflies has
/// not sent is taken as settled, so a meeting without one is never waited for.
fn is_settled(status: Option<&str>) -> bool {
    !matches!(status, Some("not_started" | "started"))
}

/// The meetings a listing answers, newest first, from `meetings` as Fireflies sent them, oldest
/// first: every one up to the first still being transcribed, which the next listing starts from.
/// One transcribing for longer than [`TRANSCRIBING_PATIENCE`] is taken as it is.
fn settled(meetings: Vec<NoteMeeting>, now: DateTime<Utc>) -> Vec<ListedMeeting> {
    let mut listed = Vec::new();
    for meeting in meetings {
        let status = meeting.process_meeting_status.clone();
        let Some(listed_meeting) = listed_meeting(meeting) else {
            continue;
        };
        let waiting = now - listed_meeting.started_at < TRANSCRIBING_PATIENCE;
        if !is_settled(status.as_deref()) && waiting {
            break;
        }
        listed.push(listed_meeting);
    }
    listed.reverse();
    listed
}

/// A listed meeting, or `None` for one without a start time Elysium can read.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a meeting's length in milliseconds is far inside i64"
)]
fn listed_meeting(meeting: NoteMeeting) -> Option<ListedMeeting> {
    let started_at = time_of(&meeting.date)?;
    let participants = match meeting.all_emails {
        Value::Array(addresses) => addresses
            .iter()
            .filter_map(|address| address.as_str())
            .map(str::trim)
            .filter(|address| !address.is_empty())
            .map(str::to_owned)
            .collect(),
        Value::String(addresses) => addresses
            .split(',')
            .map(str::trim)
            .filter(|address| !address.is_empty())
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    };
    Some(ListedMeeting {
        url: Some(format!("{MEETING_PAGE}{}", meeting.id)),
        external_id: meeting.id,
        title: meeting.title.unwrap_or_default(),
        started_at,
        duration_ms: meeting
            .duration_mins
            .filter(|minutes| minutes.is_finite() && *minutes >= 0.0)
            .map(|minutes| (minutes * 60_000.0).round() as i64),
        organizer: meeting.creator_email.unwrap_or_default(),
        participants,
    })
}

/// A time sent as milliseconds since the epoch, or as ISO 8601 text.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a finite time in milliseconds saturates at i64's bounds, which no timestamp reaches"
)]
fn time_of(value: &Value) -> Option<DateTime<Utc>> {
    match value {
        Value::Number(number) => number
            .as_f64()
            .filter(|milliseconds| milliseconds.is_finite())
            .and_then(|milliseconds| DateTime::from_timestamp_millis(milliseconds.round() as i64)),
        Value::String(text) => text.trim().parse::<i64>().map_or_else(
            |_not_a_number| {
                DateTime::parse_from_rfc3339(text.trim())
                    .ok()
                    .map(|time| time.with_timezone(&Utc))
            },
            DateTime::from_timestamp_millis,
        ),
        _ => None,
    }
}

/// A meeting's captions and the edits saved on them, as the meeting page's query sends them.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MeetingNote {
    #[serde(default)]
    process_meeting_status: Option<String>,
    /// Speaker names, keyed one past the caption's speaker id.
    #[serde(default)]
    speaker_meta: Option<HashMap<String, Value>>,
    /// Edited sentences by caption index; an empty one was deleted.
    #[serde(default)]
    sentence_meta: Option<HashMap<String, Value>>,
    /// The caption indexes reassigned to each speaker id.
    #[serde(default)]
    label_meta: Option<HashMap<String, Value>>,
    #[serde(default)]
    captions: Option<Vec<Caption>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Caption {
    #[serde(default)]
    sentence: Option<String>,
    #[serde(default, rename = "speaker_id")]
    speaker_id: Option<i64>,
    /// Seconds from the start.
    #[serde(default)]
    time: Option<f64>,
    #[serde(default)]
    end_time: Option<f64>,
}

/// A meeting's lines, in order, as Fireflies shows them: each edited sentence in place of what
/// was heard, deleted ones left out, reassigned ones under their new speaker, and captions with
/// neither a letter nor a digit, which Fireflies hides, left out too.
fn transcript_lines(note: MeetingNote) -> Vec<TranscriptLine> {
    let speaker_names = note.speaker_meta.unwrap_or_default();
    let edits = note.sentence_meta.unwrap_or_default();
    let mut reassigned: HashMap<u64, i64> = HashMap::new();
    for (speaker_id, indexes) in note.label_meta.unwrap_or_default() {
        let Ok(speaker_id) = speaker_id.parse::<i64>() else {
            continue;
        };
        for index in indexes.as_array().into_iter().flatten() {
            if let Some(index) = index.as_u64() {
                reassigned.insert(index, speaker_id);
            }
        }
    }

    let mut lines = Vec::new();
    for (index, caption) in note.captions.unwrap_or_default().into_iter().enumerate() {
        let index_key = index.to_string();
        let text = edits
            .get(&index_key)
            .and_then(Value::as_str)
            .map_or_else(|| caption.sentence.unwrap_or_default(), str::to_owned);
        let text = text.trim();
        if !text.chars().any(char::is_alphanumeric) {
            continue;
        }
        let Some(start_ms) = caption.time.and_then(seconds_to_milliseconds) else {
            continue;
        };
        let end_ms = caption
            .end_time
            .and_then(seconds_to_milliseconds)
            .filter(|end_ms| *end_ms >= start_ms)
            .unwrap_or(start_ms);
        let speaker_id = u64::try_from(index)
            .ok()
            .and_then(|index| reassigned.get(&index).copied())
            .or(caption.speaker_id);
        lines.push(TranscriptLine {
            start_ms,
            end_ms,
            speaker: speaker_id.map_or_else(String::new, |speaker_id| {
                speaker_name(&speaker_names, speaker_id)
            }),
            text: text.to_owned(),
        });
    }
    lines
}

/// What Fireflies calls `speaker_id`: its name, keyed one past the id, or `Speaker <id + 1>`
/// as the web app shows a speaker nobody named.
fn speaker_name(names: &HashMap<String, Value>, speaker_id: i64) -> String {
    let number = speaker_id.saturating_add(1);
    names
        .get(&number.to_string())
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map_or_else(|| format!("Speaker {number}"), str::to_owned)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "a position inside a meeting, in milliseconds, is far inside i64"
)]
fn seconds_to_milliseconds(seconds: f64) -> Option<i64> {
    (seconds.is_finite() && seconds >= 0.0).then(|| (seconds * 1000.0).round() as i64)
}

#[cfg(test)]
mod tests;
