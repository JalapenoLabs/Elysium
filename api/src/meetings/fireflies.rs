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
//! - One meeting, in two calls of `transcript`: first its sentences, each with a speaker and
//!   start and end times in seconds, then a download address for its video or, failing that,
//!   its audio. Both addresses expire after a day, so they are asked for right before the
//!   download.
//!
//! Recording addresses need a Pro plan or higher, and video needs the "record meeting video"
//! setting in Fireflies. On a lower plan Fireflies refuses the second call with
//! `paid_required`, which is read as a meeting without a recording: it is processed from its
//! transcript alone. The sentences are asked for on their own so that refusal never costs the
//! transcript, and any refusal of the sentences still fails the fetch.
//!
//! Nothing Fireflies' own AI wrote is asked for: no summary, action items, notes, outline, or
//! analytics, since the model behind them is unknown. The queries name what they read, field
//! by field, and a test holds them to it.
//!
//! # Limits
//!
//! Fireflies allows 30 to 60 requests a minute and, below Business, 500 to 5,000 a day.
//! Elysium lists every ten minutes (`crate::meetings::POLL_INTERVAL`) and fetches each new
//! meeting once, in two calls, well inside the smallest of them.

use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use reqwest::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{Level, event};
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
const MAX_LISTING: u32 = 50;

const LIST_QUERY: &str = "query Meetings($fromDate: DateTime, $limit: Int) {
  transcripts(fromDate: $fromDate, limit: $limit) {
    id title date duration organizer_email participants transcript_url
  }
}";

/// A meeting's sentences, which every plan reads.
const TRANSCRIPT_QUERY: &str = "query MeetingTranscript($id: String!) {
  transcript(id: $id) {
    id
    sentences { start_time end_time speaker_name text }
  }
}";

/// A meeting's recording addresses, asked for apart from its sentences because Fireflies
/// gates them by plan, so a refusal here leaves the transcript read.
const RECORDING_QUERY: &str = "query MeetingRecording($id: String!) {
  transcript(id: $id) { id video_url audio_url }
}";

/// The code Fireflies answers when the account's plan does not include what was asked, from
/// its documented error codes (`docs.fireflies.ai`, Error codes).
const PAID_REQUIRED: &str = "paid_required";

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

    /// A client of a fake Fireflies listening at `endpoint`.
    #[cfg(test)]
    pub fn at(http: reqwest::Client, endpoint: Url) -> Self {
        Self { http, endpoint }
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
    ) -> Result<Value, QueryFailure> {
        let response = self
            .http
            .post(self.endpoint.clone())
            .bearer_auth(credential.expose_secret())
            .timeout(REQUEST_TIMEOUT)
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await
            .map_err(|error| {
                QueryFailure::Other(SourceError::Refused(format!(
                    "Fireflies could not be reached: {error}"
                )))
            })?;
        let status = response.status();
        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            return Err(QueryFailure::Other(unauthorized()));
        }
        let body: Value = response.json().await.map_err(|error| {
            QueryFailure::Other(SourceError::Refused(format!(
                "Fireflies answered {status} with a body that is not JSON: {error}"
            )))
        })?;
        if let Some(error) = body["errors"].as_array().and_then(|errors| errors.first()) {
            return Err(graphql_error(error));
        }
        if !status.is_success() {
            return Err(QueryFailure::Other(SourceError::Refused(format!(
                "Fireflies answered {status}"
            ))));
        }
        Ok(body["data"].clone())
    }

    /// A meeting's video, else its audio, or `None` when Fireflies offers neither.
    ///
    /// A plan below Pro is answered `paid_required`, which means the account keeps no
    /// recording Elysium may download: the meeting goes on from its transcript alone. Any other
    /// failure fails the fetch, so a Fireflies that did not answer never drops a recording.
    async fn fetch_recording(
        &self,
        credential: &SecretString,
        external_id: &str,
    ) -> Result<Option<RecordingSource>, SourceError> {
        let variables = json!({ "id": external_id });
        let data = match self.query(credential, RECORDING_QUERY, variables).await {
            Ok(data) => data,
            Err(QueryFailure::PaidPlanRequired { tier, .. }) => {
                event!(
                    name: "meetings.fireflies.recording.paid_plan_required",
                    Level::INFO,
                    meeting.external_id = external_id,
                    fireflies.tier = %tier,
                    "Fireflies keeps no recording on this account's plan, so the meeting goes on \
                     from its transcript alone",
                );
                return Ok(None);
            }
            Err(QueryFailure::Other(error)) => return Err(error),
        };
        // A meeting removed between the two calls has no recording left to download.
        let media: Option<FetchedMedia> = serde_json::from_value(data["transcript"].clone())
            .map_err(|error| {
                SourceError::Refused(format!(
                    "Fireflies sent a meeting's recording in an unknown shape: {error}"
                ))
            })?;
        Ok(media.and_then(recording_of))
    }
}

impl MeetingSource for Fireflies {
    /// The person's API key.
    type SignIn = SecretString;

    async fn list(
        &self,
        sign_in: &SecretString,
        since: Option<DateTime<Utc>>,
        limit: u32,
    ) -> Result<Vec<ListedMeeting>, SourceError> {
        let variables = json!({
            "fromDate": since.map(|since| since.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            "limit": limit.min(MAX_LISTING),
        });
        let data = self.query(sign_in, LIST_QUERY, variables).await?;
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
        sign_in: &SecretString,
        external_id: &str,
    ) -> Result<FetchedMeeting, SourceError> {
        let data = self
            .query(sign_in, TRANSCRIPT_QUERY, json!({ "id": external_id }))
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
        Ok(FetchedMeeting {
            transcript: transcript_lines(transcript),
            recording: self.fetch_recording(sign_in, external_id).await?,
        })
    }
}

/// Why a query answered no data.
#[derive(Debug)]
enum QueryFailure {
    /// Fireflies answered `paid_required`: the account's plan does not include what was
    /// asked. `tier` is the plan Fireflies names, such as `pro_or_higher`, and `refusal` is
    /// the failure for a caller that cannot go on without what was asked.
    PaidPlanRequired { tier: String, refusal: SourceError },
    /// Any other failure, as the meeting source reports it.
    Other(SourceError),
}

impl From<QueryFailure> for SourceError {
    fn from(failure: QueryFailure) -> Self {
        match failure {
            QueryFailure::PaidPlanRequired { refusal, .. } => refusal,
            QueryFailure::Other(error) => error,
        }
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

/// One meeting's sentences, as Fireflies sends them.
#[derive(Debug, Deserialize)]
struct FetchedTranscript {
    #[serde(default)]
    sentences: Option<Vec<Sentence>>,
}

/// One meeting's recording addresses, as Fireflies sends them.
#[derive(Debug, Deserialize)]
struct FetchedMedia {
    #[serde(default)]
    video_url: Option<String>,
    #[serde(default)]
    audio_url: Option<String>,
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

/// A fetched transcript's lines, in order.
fn transcript_lines(transcript: FetchedTranscript) -> Vec<TranscriptLine> {
    transcript
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
        .collect()
}

/// A meeting's video, else its audio, skipping any address that is not a public download.
fn recording_of(media: FetchedMedia) -> Option<RecordingSource> {
    [
        (media.video_url, RecordingKind::Video),
        (media.audio_url, RecordingKind::Audio),
    ]
    .into_iter()
    .find_map(|(address, kind)| {
        let address = address?;
        let parsed = Url::parse(&address).ok()?;
        is_public_download(&parsed).then(|| RecordingSource {
            url: SecretString::from(address),
            kind,
        })
    })
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

/// A GraphQL error as a [`QueryFailure`], by the code Fireflies puts in its extensions.
fn graphql_error(error: &Value) -> QueryFailure {
    let code = error["extensions"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let message = error["message"]
        .as_str()
        .unwrap_or("Fireflies refused the request");
    let refusal = SourceError::Refused(format!("Fireflies refused: {message}"));
    if code == PAID_REQUIRED {
        let tier = error["extensions"]["metadata"]["tier"]
            .as_str()
            .unwrap_or("paid")
            .to_owned();
        return QueryFailure::PaidPlanRequired { tier, refusal };
    }
    if code.contains("auth") || code.contains("forbidden") || code.contains("unauthenticated") {
        return QueryFailure::Other(unauthorized());
    }
    if code.contains("not_found") {
        return QueryFailure::Other(SourceError::NotFound);
    }
    QueryFailure::Other(refusal)
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
            "sentences": [
                { "start_time": 0.48, "end_time": 3.1, "speaker_name": "Ada", "text": " Hi all. " },
                { "start_time": "3.5", "end_time": "2.0", "speaker_name": null, "text": "Launch is Friday." },
                { "start_time": null, "end_time": 9, "speaker_name": "Ada", "text": "lost" },
                { "start_time": 10, "end_time": 11, "speaker_name": "Ada", "text": "   " },
            ],
        }))
        .expect("parses");

        assert_eq!(
            transcript_lines(transcript),
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
    }

    #[test]
    fn video_is_preferred_and_an_unsafe_address_is_ignored() {
        let recording_kind = |media: Value| {
            let media: FetchedMedia = serde_json::from_value(media).expect("parses");
            recording_of(media).map(|recording| recording.kind)
        };

        assert_eq!(
            recording_kind(json!({
                "video_url": "https://cdn.fireflies.ai/v.mp4",
                "audio_url": "https://cdn.fireflies.ai/a.mp3",
            })),
            Some(RecordingKind::Video)
        );
        assert_eq!(
            recording_kind(json!({
                "video_url": null,
                "audio_url": "https://cdn.fireflies.ai/a.mp3?sig=1",
            })),
            Some(RecordingKind::Audio)
        );
        assert_eq!(
            recording_kind(json!({
                "video_url": "http://169.254.169.254/latest/meta-data",
                "audio_url": null,
            })),
            None
        );
    }

    /// What Fireflies' own AI writes, by the names its GraphQL schema gives them.
    const VENDOR_AI_FIELDS: [&str; 18] = [
        "summary",
        "action_items",
        "overview",
        "short_overview",
        "short_summary",
        "notes",
        "keywords",
        "outline",
        "shorthand_bullet",
        "bullet_gist",
        "gist",
        "topics_discussed",
        "meeting_type",
        "transcript_chapters",
        "extended_sections",
        "analytics",
        "sentiments",
        "ai_filters",
    ];

    #[test]
    fn no_query_asks_for_anything_fireflies_ai_wrote() {
        for query in [LIST_QUERY, TRANSCRIPT_QUERY, RECORDING_QUERY, WHOAMI_QUERY] {
            let fields = fields_of(query);
            for vendor_field in VENDOR_AI_FIELDS {
                assert!(
                    !fields.contains(&vendor_field),
                    "a Fireflies query asks for {vendor_field}: {query}"
                );
            }
        }
    }

    /// The names a query is made of, split on everything that cannot be part of one.
    fn fields_of(query: &str) -> Vec<&str> {
        query
            .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .filter(|field| !field.is_empty())
            .collect()
    }

    /// Fireflies refuses recording addresses below Pro, so asking for them beside the
    /// sentences would cost the transcript on those plans.
    #[test]
    fn the_transcript_query_asks_for_no_recording_address() {
        let fields = fields_of(TRANSCRIPT_QUERY);
        for media_field in ["video_url", "audio_url"] {
            assert!(
                !fields.contains(&media_field),
                "the transcript query asks for {media_field}"
            );
        }
    }

    #[test]
    fn graphql_errors_are_classified_by_their_code() {
        let classified = |error: Value| SourceError::from(graphql_error(&error));
        assert!(matches!(
            classified(json!({ "message": "no", "extensions": { "code": "auth_failed" } })),
            SourceError::Unauthorized(_)
        ));
        assert!(matches!(
            classified(json!({ "message": "gone", "extensions": { "code": "object_not_found" } })),
            SourceError::NotFound
        ));
        assert!(matches!(
            classified(json!({ "message": "slow down", "extensions": { "code": "too_many_requests" } })),
            SourceError::Refused(message) if message.contains("slow down")
        ));
        assert!(matches!(
            graphql_error(&paid_required()["errors"][0]),
            QueryFailure::PaidPlanRequired { tier, refusal: SourceError::Refused(message) }
                if tier == "pro_or_higher" && message.contains("paid plan")
        ));
    }

    /// Fireflies' answer to a free plan asking for what needs Pro, in the shape its error
    /// codes document.
    fn paid_required() -> Value {
        json!({
            "data": null,
            "errors": [{
                "message": "You need to be subscribed to a paid plan to perform this action",
                "code": "paid_required",
                "extensions": {
                    "code": "paid_required",
                    "status": 403,
                    "metadata": { "tier": "pro_or_higher" },
                },
            }],
        })
    }

    /// A meeting's sentences, as Fireflies answers the transcript query.
    fn sentences() -> Value {
        json!({
            "data": {
                "transcript": {
                    "id": "01HX",
                    "sentences": [
                        { "start_time": 0.5, "end_time": 2.0, "speaker_name": "Ada", "text": "Launch is Friday." },
                    ],
                },
            },
        })
    }

    /// What a fake Fireflies answers to each of the meeting's two queries.
    #[derive(Clone)]
    struct Answers {
        transcript: Value,
        recording: Value,
    }

    /// A client of a fake Fireflies that answers the transcript and recording queries with
    /// `answers`, over HTTP 200 as Fireflies sends its GraphQL errors.
    async fn fake_fireflies(answers: Answers) -> Fireflies {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        let endpoint = Url::parse(&format!("http://{address}/graphql")).expect("a url");
        let router = axum::Router::new()
            .fallback(answer_fireflies)
            .with_state(answers);
        tokio::spawn(async move { axum::serve(listener, router).await });
        Fireflies::at(reqwest::Client::new(), endpoint)
    }

    async fn answer_fireflies(
        axum::extract::State(answers): axum::extract::State<Answers>,
        axum::Json(request): axum::Json<Value>,
    ) -> axum::Json<Value> {
        let query = request["query"].as_str().unwrap_or_default();
        if query == RECORDING_QUERY {
            return axum::Json(answers.recording);
        }
        assert_eq!(
            query, TRANSCRIPT_QUERY,
            "the fake knows only the meeting's queries"
        );
        axum::Json(answers.transcript)
    }

    /// Regression: every meeting of an account on Fireflies' free plan failed `fetch_meeting`
    /// with "You need to be subscribed to a paid plan", because the recording addresses, which
    /// need Pro, were asked for in the same query as the sentences.
    #[tokio::test]
    async fn a_refused_recording_leaves_the_transcript_fetched_with_no_recording() {
        let fireflies = fake_fireflies(Answers {
            transcript: sentences(),
            recording: paid_required(),
        })
        .await;

        let meeting = fireflies
            .fetch(&SecretString::from("fireflies-key"), "01HX")
            .await
            .expect("the transcript is fetched");

        assert_eq!(
            meeting.transcript,
            vec![TranscriptLine {
                start_ms: 500,
                end_ms: 2000,
                speaker: "Ada".to_owned(),
                text: "Launch is Friday.".to_owned(),
            }]
        );
        assert!(meeting.recording.is_none(), "the plan keeps no recording");
    }

    #[tokio::test]
    async fn a_refused_transcript_still_fails_the_fetch() {
        let fireflies = fake_fireflies(Answers {
            transcript: paid_required(),
            recording: paid_required(),
        })
        .await;

        let failure = fireflies
            .fetch(&SecretString::from("fireflies-key"), "01HX")
            .await
            .map(|meeting| meeting.transcript.len());

        assert!(
            matches!(&failure, Err(SourceError::Refused(message)) if message.contains("paid plan")),
            "a transcript Fireflies refuses fails the fetch: {failure:?}"
        );
    }

    #[tokio::test]
    async fn an_offered_recording_is_fetched_beside_the_transcript() {
        let fireflies = fake_fireflies(Answers {
            transcript: sentences(),
            recording: json!({
                "data": {
                    "transcript": {
                        "id": "01HX",
                        "video_url": null,
                        "audio_url": "https://cdn.fireflies.ai/a.mp3",
                    },
                },
            }),
        })
        .await;

        let meeting = fireflies
            .fetch(&SecretString::from("fireflies-key"), "01HX")
            .await
            .expect("the meeting is fetched");

        assert_eq!(meeting.transcript.len(), 1);
        assert_eq!(
            meeting.recording.map(|recording| recording.kind),
            Some(RecordingKind::Audio)
        );
    }
}
