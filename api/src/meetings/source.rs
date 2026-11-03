// Copyright © 2026 Jalapeno Labs

//! What a meeting source reads: a listing of meetings, and one meeting's transcript and
//! recording.
//!
//! A source is the meeting counterpart of `crate::connectors::Connector`: it reads as its
//! account's person, signed in the way its connector signs in. Today's source is Fireflies,
//! read with the person's own API key; sources that read Google Meet and Teams recordings
//! through a signed-in browser fill the same trait.

use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use url::Url;

/// One meeting as a listing shows it, before its transcript is read.
#[derive(Debug, Clone, PartialEq)]
pub struct ListedMeeting {
    /// The source's own id for it, unique within the account.
    pub external_id: String,
    pub title: String,
    pub started_at: DateTime<Utc>,
    pub duration_ms: Option<i64>,
    pub organizer: String,
    pub participants: Vec<String>,
    /// The meeting's page at its source.
    pub url: Option<String>,
}

/// One line of a transcript: what one speaker said, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TranscriptLine {
    /// From the start of the recording.
    pub start_ms: i64,
    pub end_ms: i64,
    /// Who said it, as the source names them; empty when it does not know.
    pub speaker: String,
    pub text: String,
}

/// Where a meeting's recording can be downloaded from, for a short while.
#[derive(Debug, Clone)]
pub struct RecordingSource {
    /// A signed address: anyone holding it can download the recording until it expires, so
    /// it is treated as a secret and never logged.
    pub url: SecretString,
    /// Whether it is the video or only the audio, which decides the clips' players.
    pub kind: RecordingKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingKind {
    Video,
    Audio,
}

/// One meeting with everything processing needs.
#[derive(Debug, Clone)]
pub struct FetchedMeeting {
    pub transcript: Vec<TranscriptLine>,
    /// `None` when the source keeps no recording of it, or the account's plan does not
    /// include recordings.
    pub recording: Option<RecordingSource>,
}

/// A source refused a call or could not be reached.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SourceError {
    /// The key or session is not accepted. The message says what to check.
    #[error("{0}")]
    Unauthorized(String),
    /// The source has no such meeting, or the account cannot see it.
    #[error("the meeting source has no such meeting, or this account cannot see it")]
    NotFound,
    /// The source could not be reached, refused, or answered something Elysium does not
    /// understand.
    #[error("{0}")]
    Refused(String),
}

/// A service that records meetings, read as its account's person.
pub trait MeetingSource {
    /// Meetings that started at or after `since`, newest first, at most `limit` of them.
    fn list(
        &self,
        credential: &SecretString,
        since: Option<DateTime<Utc>>,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<ListedMeeting>, SourceError>> + Send;

    /// One meeting's transcript and a fresh address to download its recording from.
    fn fetch(
        &self,
        credential: &SecretString,
        external_id: &str,
    ) -> impl Future<Output = Result<FetchedMeeting, SourceError>> + Send;
}

/// Whether `url` is an address a source may hand over for a download: `https`, and not a
/// literal address, so a recording link can never point Elysium at its own network.
pub fn is_public_download(url: &Url) -> bool {
    url.scheme() == "https"
        && matches!(url.host(), Some(url::Host::Domain(domain)) if domain.contains('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_on_a_named_host_is_downloaded() {
        let accepted = |text: &str| is_public_download(&Url::parse(text).expect("a url"));
        assert!(accepted("https://cdn.fireflies.ai/recordings/a.mp3?sig=x"));
        assert!(!accepted("http://cdn.fireflies.ai/a.mp3"));
        assert!(!accepted("https://10.0.0.5/a.mp3"));
        assert!(!accepted("https://[::1]/a.mp3"));
        assert!(!accepted("https://api/a.mp3"));
        assert!(!accepted("file:///etc/passwd"));
    }
}
