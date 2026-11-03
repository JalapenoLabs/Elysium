// Copyright © 2026 Jalapeno Labs

//! Fireflies.ai, read through its GraphQL API with the person's own API key.
//!
//! A Fireflies API key belongs to one user and reads what that user can see, so nothing is
//! installed into an organization: the person copies the key from Fireflies' Integrations
//! page and Elysium reads as them. `api.fireflies.ai` is fixed in code.
//!
//! # What is read
//!
//! - A listing (`transcripts`) of the meetings that started since the account's cursor.
//! - One meeting (`transcript`): its sentences, each with a speaker and start and end times
//!   in seconds, and a download address for its video or, failing that, its audio. Both
//!   addresses expire after a day, so they are asked for right before the download.
//!
//! Recordings need a Pro plan or higher, and video needs the "record meeting video" setting
//! in Fireflies; without them the meeting is processed from its transcript alone.
//!
//! # Limits
//!
//! Fireflies allows 30 to 60 requests a minute and, below Business, 500 to 5,000 a day.
//! Elysium lists every ten minutes (`crate::meetings::POLL_INTERVAL`) and fetches each new
//! meeting once, well inside the smallest of them.

use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use reqwest::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use super::source::{
    FetchedMeeting, ListedMeeting, MeetingSource, RecordingKind, RecordingSource, SourceError,
    TranscriptLine, is_public_download,
};

/// Fireflies' GraphQL endpoint.
const ENDPOINT: &str = "https://api.fireflies.ai/graphql";

/// How long one call may take. A transcript of a long meeting is a few megabytes.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// The most meetings one listing asks for; Fireflies refuses more than 50.
pub const MAX_LISTING: u32 = 50;

const LIST_QUERY: &str = "query Meetings($fromDate: DateTime, $limit: Int) {
  transcripts(fromDate: $fromDate, limit: $limit) {
    id title date duration organizer_email participants transcript_url
  }
}";

const FETCH_QUERY: &str = "query Meeting($id: String!) {
  transcript(id: $id) {
    id video_url audio_url
    sentences { start_time end_time speaker_name text }
  }
}";

const WHOAMI_QUERY: &str = "query { user { email name } }";

/// Whose key it is, as Fireflies reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirefliesUser {
    pub email: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct Fireflies {
    http: reqwest::Client,
    endpoint: Url,
}

impl Fireflies {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            endpoint: Url::parse(ENDPOINT).expect("the Fireflies endpoint is a valid URL"),
        }
    }

    /// Whose key `credential` is, which also proves Fireflies accepts it.
    ///
    /// # Errors
    /// [`SourceError::Unauthorized`] when Fireflies refuses the key.
    pub async fn whoami(&self, credential: &SecretString) -> Result<FirefliesUser, SourceError> {
        let data = self.query(credential, WHOAMI_QUERY, json!({})).await?;
        let user = &data["user"];
        let email = user["email"].as_str().unwrap_or_default().to_owned();
        if email.is_empty() {
            return Err(SourceError::Refused(
                "Fireflies answered without the key's user".to_owned(),
            ));
        }
        Ok(FirefliesUser {
            email,
            name: user["name"].as_str().unwrap_or_default().to_owned(),
        })
    }

    /// Runs one GraphQL query and returns its `data`.
    async fn query(
        &self,
        credential: &SecretString,
        query: &str,
        variables: Value,
    ) -> Result<Value, SourceError> {
        let response = self
            .http
            .post(self.endpoint.clone())
            .bearer_auth(credential.expose_secret())
            .timeout(REQUEST_TIMEOUT)
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await
            .map_err(|error| {
                SourceError::Refused(format!("Fireflies could not be reached: {error}"))
            })?;
        let status = response.status();
        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            return Err(unauthorized());
        }
        let body: Value = response.json().await.map_err(|error| {
            SourceError::Refused(format!(
                "Fireflies answered {status} with a body that is not JSON: {error}"
            ))
        })?;
        if let Some(error) = body["errors"].as_array().and_then(|errors| errors.first()) {
            return Err(graphql_error(error));
        }
        if !status.is_success() {
            return Err(SourceError::Refused(format!("Fireflies answered {status}")));
        }
        Ok(body["data"].clone())
    }
}

impl MeetingSource for Fireflies {
    async fn list(
        &self,
        credential: &SecretString,
        since: Option<DateTime<Utc>>,
        limit: u32,
    ) -> Result<Vec<ListedMeeting>, SourceError> {
        let variables = json!({
            "fromDate": since.map(|since| since.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            "limit": limit.min(MAX_LISTING),
        });
        let data = self.query(credential, LIST_QUERY, variables).await?;
        let transcripts: Vec<ListedTranscript> =
            serde_json::from_value(data["transcripts"].clone()).map_err(|error| {
                SourceError::Refused(format!(
                    "Fireflies listed meetings in an unknown shape: {error}"
                ))
            })?;
        Ok(transcripts.into_iter().filter_map(listed).collect())
    }

    async fn fetch(
        &self,
        credential: &SecretString,
        external_id: &str,
    ) -> Result<FetchedMeeting, SourceError> {
        let data = self
            .query(credential, FETCH_QUERY, json!({ "id": external_id }))
            .await?;
        if data["transcript"].is_null() {
            return Err(SourceError::NotFound);
        }
        let transcript: FetchedTranscript = serde_json::from_value(data["transcript"].clone())
            .map_err(|error| {
                SourceError::Refused(format!(
                    "Fireflies sent a meeting in an unknown shape: {error}"
                ))
            })?;
        Ok(fetched(transcript))
    }
}

/// A listed transcript, as Fireflies sends it.
#[derive(Debug, Deserialize)]
struct ListedTranscript {
    id: String,
    #[serde(default)]
    title: Option<String>,
    /// Milliseconds since the epoch.
    #[serde(default)]
    date: Option<f64>,
    /// Minutes.
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    organizer_email: Option<String>,
    #[serde(default)]
    participants: Option<Vec<String>>,
    #[serde(default)]
    transcript_url: Option<String>,
}

/// One meeting's sentences and recordings, as Fireflies sends them.
#[derive(Debug, Deserialize)]
struct FetchedTranscript {
    #[serde(default)]
    video_url: Option<String>,
    #[serde(default)]
    audio_url: Option<String>,
    #[serde(default)]
    sentences: Option<Vec<Sentence>>,
}

#[derive(Debug, Deserialize)]
struct Sentence {
    /// Seconds from the start, sent as a number or as a numeric string.
    #[serde(default)]
    start_time: Value,
    #[serde(default)]
    end_time: Value,
    #[serde(default)]
    speaker_name: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

/// A listed transcript as a meeting, or `None` for one without a usable start time.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a meeting's length in milliseconds is far inside i64"
)]
fn listed(transcript: ListedTranscript) -> Option<ListedMeeting> {
    let started_at = transcript.date.and_then(milliseconds_to_time)?;
    Some(ListedMeeting {
        external_id: transcript.id,
        title: transcript.title.unwrap_or_default(),
        started_at,
        duration_ms: transcript
            .duration
            .filter(|minutes| minutes.is_finite() && *minutes >= 0.0)
            .map(|minutes| (minutes * 60_000.0).round() as i64),
        organizer: transcript.organizer_email.unwrap_or_default(),
        participants: transcript.participants.unwrap_or_default(),
        url: transcript.transcript_url,
    })
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "a finite time in milliseconds saturates at i64's bounds, which no timestamp reaches"
)]
fn milliseconds_to_time(milliseconds: f64) -> Option<DateTime<Utc>> {
    if !milliseconds.is_finite() {
        return None;
    }
    Utc.timestamp_millis_opt(milliseconds.round() as i64)
        .single()
}

/// A fetched transcript as a meeting: its lines in order, and its video, else its audio.
fn fetched(transcript: FetchedTranscript) -> FetchedMeeting {
    let lines = transcript
        .sentences
        .unwrap_or_default()
        .into_iter()
        .filter_map(|sentence| {
            let start_ms = seconds_to_milliseconds(&sentence.start_time)?;
            let end_ms = seconds_to_milliseconds(&sentence.end_time)
                .filter(|end_ms| *end_ms >= start_ms)
                .unwrap_or(start_ms);
            let text = sentence.text.unwrap_or_default().trim().to_owned();
            if text.is_empty() {
                return None;
            }
            Some(TranscriptLine {
                start_ms,
                end_ms,
                speaker: sentence.speaker_name.unwrap_or_default(),
                text,
            })
        })
        .collect();

    let recording = [
        (transcript.video_url, RecordingKind::Video),
        (transcript.audio_url, RecordingKind::Audio),
    ]
    .into_iter()
    .find_map(|(address, kind)| {
        let address = address?;
        let parsed = Url::parse(&address).ok()?;
        is_public_download(&parsed).then(|| RecordingSource {
            url: SecretString::from(address),
            kind,
        })
    });

    FetchedMeeting {
        transcript: lines,
        recording,
    }
}

/// Seconds, as a number or a numeric string, in whole milliseconds.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a position inside a meeting, in milliseconds, is far inside i64"
)]
fn seconds_to_milliseconds(value: &Value) -> Option<i64> {
    let seconds = match value {
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => text.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    (seconds.is_finite() && seconds >= 0.0).then(|| (seconds * 1000.0).round() as i64)
}

fn unauthorized() -> SourceError {
    SourceError::Unauthorized(
        "Fireflies did not accept the API key; copy it again from Fireflies' Integrations page"
            .to_owned(),
    )
}

/// A GraphQL error as a [`SourceError`], by the code Fireflies puts in its extensions.
fn graphql_error(error: &Value) -> SourceError {
    let code = error["extensions"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let message = error["message"]
        .as_str()
        .unwrap_or("Fireflies refused the request")
        .to_owned();
    if code.contains("auth") || code.contains("forbidden") || code.contains("unauthenticated") {
        return unauthorized();
    }
    if code.contains("not_found") {
        return SourceError::NotFound;
    }
    SourceError::Refused(format!("Fireflies refused: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_reads_times_in_milliseconds_and_durations_in_minutes() {
        let transcript: ListedTranscript = serde_json::from_value(json!({
            "id": "01HX",
            "title": "Weekly sync",
            "date": 1_761_000_000_000.0,
            "duration": 42.5,
            "organizer_email": "ada@example.com",
            "participants": ["ada@example.com", "grace@example.com"],
            "transcript_url": "https://app.fireflies.ai/view/01HX",
        }))
        .expect("parses");
        let meeting = listed(transcript).expect("a meeting");

        assert_eq!(meeting.external_id, "01HX");
        assert_eq!(meeting.started_at.timestamp_millis(), 1_761_000_000_000);
        assert_eq!(meeting.duration_ms, Some(2_550_000));
        assert_eq!(meeting.participants.len(), 2);
    }

    #[test]
    fn a_listed_meeting_without_a_date_is_skipped() {
        let transcript: ListedTranscript =
            serde_json::from_value(json!({ "id": "x" })).expect("parses");
        assert_eq!(listed(transcript), None);
    }

    #[test]
    fn sentences_become_lines_with_times_as_numbers_or_strings() {
        let transcript: FetchedTranscript = serde_json::from_value(json!({
            "video_url": null,
            "audio_url": "https://cdn.fireflies.ai/a.mp3?sig=1",
            "sentences": [
                { "start_time": 0.48, "end_time": 3.1, "speaker_name": "Ada", "text": " Hi all. " },
                { "start_time": "3.5", "end_time": "2.0", "speaker_name": null, "text": "Launch is Friday." },
                { "start_time": null, "end_time": 9, "speaker_name": "Ada", "text": "lost" },
                { "start_time": 10, "end_time": 11, "speaker_name": "Ada", "text": "   " },
            ],
        }))
        .expect("parses");
        let meeting = fetched(transcript);

        assert_eq!(
            meeting.transcript,
            vec![
                TranscriptLine {
                    start_ms: 480,
                    end_ms: 3100,
                    speaker: "Ada".to_owned(),
                    text: "Hi all.".to_owned(),
                },
                // An end before its start is taken as the start.
                TranscriptLine {
                    start_ms: 3500,
                    end_ms: 3500,
                    speaker: String::new(),
                    text: "Launch is Friday.".to_owned(),
                },
            ]
        );
        let recording = meeting.recording.expect("the audio");
        assert_eq!(recording.kind, RecordingKind::Audio);
    }

    #[test]
    fn video_is_preferred_and_an_unsafe_address_is_ignored() {
        let transcript: FetchedTranscript = serde_json::from_value(json!({
            "video_url": "https://cdn.fireflies.ai/v.mp4",
            "audio_url": "https://cdn.fireflies.ai/a.mp3",
        }))
        .expect("parses");
        assert_eq!(
            fetched(transcript)
                .recording
                .map(|recording| recording.kind),
            Some(RecordingKind::Video)
        );

        let transcript: FetchedTranscript = serde_json::from_value(json!({
            "video_url": "http://169.254.169.254/latest/meta-data",
            "audio_url": null,
        }))
        .expect("parses");
        assert!(fetched(transcript).recording.is_none());
    }

    #[test]
    fn graphql_errors_are_classified_by_their_code() {
        assert!(matches!(
            graphql_error(&json!({ "message": "no", "extensions": { "code": "auth_failed" } })),
            SourceError::Unauthorized(_)
        ));
        assert!(matches!(
            graphql_error(
                &json!({ "message": "gone", "extensions": { "code": "object_not_found" } })
            ),
            SourceError::NotFound
        ));
        assert!(matches!(
            graphql_error(&json!({ "message": "slow down", "extensions": { "code": "too_many_requests" } })),
            SourceError::Refused(message) if message.contains("slow down")
        ));
    }
}
