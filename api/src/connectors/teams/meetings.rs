// Copyright © 2026 Jalapeno Labs

//! Teams as a meeting source: the person's recorded meetings, their recordings and transcripts,
//! read through the browser the connector signed in to Microsoft. See `docs/meetings.md`.
//!
//! `teams_meetings.js` runs in the browser's Teams tab, as the page. Listing reads the store Teams
//! keeps in `IndexedDB`: once a meeting's recording is ready, Teams posts a message in the
//! meeting's chat naming the recording's drive item in the organizer's `OneDrive`, its title,
//! start, and length, and the call's ended message names who organized it and took part. So the
//! meetings listed are the recorded ones whose chats this person's Teams holds. Each is known by
//! its drive item's own address on `SharePoint`'s API,
//! `https://<tenant>-my.sharepoint.com/_api/v2.1/drives/<drive>/items/<item>`, its external id.
//!
//! Fetching reads that drive item with the token Teams keeps for the `SharePoint` host: the
//! recording's pre-authenticated download address, which the fetch step downloads like any
//! source's, and the transcript as JSON, each entry naming its speaker. Offsets are from the
//! start of the transcript, which starts with the recording to within a few seconds; clips are
//! cut on the words Whisper heard, so that drift never reaches a clip. A recording that cannot be
//! downloaded (its organizer blocked downloads) is processed from its transcript alone, and the
//! fetch step says so.
//!
//! Nothing Microsoft's AI wrote (the meeting recap, notes, or summaries) is read.

use chrono::{DateTime, Duration, Utc};
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{Level, event};
use url::Url;

use super::TEAMS_ORIGINS;
use crate::browser::devtools::{Devtools, DevtoolsError};
use crate::meetings::source::{
    FetchedMeeting, ListedMeeting, MeetingSource, RecordingKind, RecordingSource, SourceError,
    TranscriptLine, is_public_download,
};
use crate::models::browser_session::BrowserSession;

const SCRIPT: &str = include_str!("../teams_meetings.js");

/// What a person sees when the browser is not signed in to Teams.
const SIGNED_OUT: &str =
    "Teams is signed out in this connector's browser; sign in again on the connector's page";

/// The longest title and organizer kept, matching `meetings`' checks on those columns.
const TITLE_MAX_CHARACTERS: usize = 2048;
const ORGANIZER_MAX_CHARACTERS: usize = 512;

/// Reads the recorded meetings of the person a Teams connector's browser is signed in as.
#[derive(Debug, Clone)]
pub struct TeamsMeetings {
    devtools: Devtools,
}

impl TeamsMeetings {
    pub const fn new(devtools: Devtools) -> Self {
        Self { devtools }
    }

    /// Runs `teams_meetings.js` with `request` in the browser's Teams tab.
    async fn evaluate(
        &self,
        browser: &BrowserSession,
        request: &Value,
    ) -> Result<Value, SourceError> {
        let port = u16::try_from(browser.port)
            .map_err(|_overflow| SourceError::Refused("a browser port out of range".to_owned()))?;
        let script = format!("({SCRIPT})({request})");
        self.devtools
            .evaluate(port, TEAMS_ORIGINS, &script)
            .await
            .map_err(|error| match error {
                DevtoolsError::NoTab(_origins) => SourceError::Refused(
                    "Teams is not open in this connector's browser yet; its chat reader opens it"
                        .to_owned(),
                ),
                other => SourceError::Refused(other.to_string()),
            })
    }
}

impl MeetingSource for TeamsMeetings {
    /// The connector's own browser, signed in to Microsoft.
    type SignIn = BrowserSession;

    async fn list(
        &self,
        sign_in: &BrowserSession,
        since: Option<DateTime<Utc>>,
        limit: u32,
    ) -> Result<Vec<ListedMeeting>, SourceError> {
        let page = self.evaluate(sign_in, &json!({ "action": "list" })).await?;
        listed(&page, since, limit)
    }

    async fn fetch(
        &self,
        sign_in: &BrowserSession,
        external_id: &str,
    ) -> Result<FetchedMeeting, SourceError> {
        let item = drive_item(external_id).ok_or(SourceError::NotFound)?;
        let request = json!({ "action": "fetch", "item": item.as_str() });
        let page = self.evaluate(sign_in, &request).await?;
        fetched(&page)
    }
}

/// The drive item `external_id` names, when it is one: `https` on a `SharePoint` host, at
/// `/_api/v2.1/drives/<drive>/items/<item>`, and nothing else, so a stored id can never point
/// the page's token anywhere but `SharePoint`'s own API.
fn drive_item(external_id: &str) -> Option<Url> {
    let url = Url::parse(external_id).ok()?;
    let on_sharepoint = url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| host.ends_with(".sharepoint.com"))
        && url.query().is_none()
        && url.fragment().is_none();
    let segments: Vec<&str> = url.path_segments()?.collect();
    let is_item = matches!(
        segments.as_slice(),
        ["_api", "v2.1", "drives", drive, "items", item] if !drive.is_empty() && !item.is_empty()
    );
    (on_sharepoint && is_item).then_some(url)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListingPage {
    signed_in: bool,
    #[serde(default)]
    found: bool,
    #[serde(default)]
    recordings: Vec<PageRecording>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageRecording {
    item: String,
    url: Option<String>,
    #[serde(default)]
    title: String,
    started_at: Option<String>,
    duration: Option<String>,
    #[serde(default)]
    organizer: String,
    #[serde(default)]
    participants: Vec<String>,
}

/// The meetings that started at or after `since` in what the listing script answered, newest
/// first, at most `limit` of them.
///
/// # Errors
/// [`SourceError::Unauthorized`] for a signed-out browser, and [`SourceError::Refused`] when the
/// page holds no store or answers in another shape.
fn listed(
    page: &Value,
    since: Option<DateTime<Utc>>,
    limit: u32,
) -> Result<Vec<ListedMeeting>, SourceError> {
    let page: ListingPage = serde_json::from_value(page.clone()).map_err(|error| {
        SourceError::Refused(format!(
            "Teams listed recordings in an unknown shape: {error}"
        ))
    })?;
    if !page.signed_in {
        return Err(SourceError::Unauthorized(SIGNED_OUT.to_owned()));
    }
    if !page.found {
        return Err(SourceError::Refused(
            "Teams is open, but its chats were not found; open a chat in its browser".to_owned(),
        ));
    }

    let mut meetings = Vec::new();
    for recording in page.recordings {
        let started_at = recording
            .started_at
            .as_deref()
            .and_then(|started_at| DateTime::parse_from_rfc3339(started_at).ok())
            .map(|started_at| started_at.with_timezone(&Utc));
        let Some(started_at) = started_at else {
            event!(
                name: "teams.meetings.recording.unlisted",
                Level::DEBUG,
                "a Teams recording without a start time was not listed",
            );
            continue;
        };
        if drive_item(&recording.item).is_none() {
            event!(
                name: "teams.meetings.recording.unlisted",
                Level::WARN,
                "a Teams recording whose drive item is not on SharePoint was not listed",
            );
            continue;
        }
        if since.is_some_and(|since| started_at < since) {
            continue;
        }
        let url = recording
            .url
            .filter(|url| Url::parse(url).is_ok_and(|parsed| parsed.scheme() == "https"));
        meetings.push(ListedMeeting {
            external_id: recording.item,
            title: truncate(&recording.title, TITLE_MAX_CHARACTERS),
            started_at,
            duration_ms: recording.duration.as_deref().and_then(clock_ms),
            organizer: truncate(&recording.organizer, ORGANIZER_MAX_CHARACTERS),
            participants: recording.participants,
            url,
        });
    }
    meetings.sort_by_key(|meeting| std::cmp::Reverse(meeting.started_at));
    meetings.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    Ok(meetings)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchPage {
    signed_in: bool,
    #[serde(default)]
    ok: bool,
    error: Option<String>,
    status: Option<u16>,
    step: Option<String>,
    recording: Option<PageDownload>,
    transcript: Option<Vec<PageEntry>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageDownload {
    url: String,
    #[serde(default)]
    is_video: bool,
}

#[derive(Deserialize)]
struct PageEntry {
    start: Option<String>,
    end: Option<String>,
    #[serde(default)]
    speaker: String,
    #[serde(default)]
    text: String,
}

/// One meeting from what the fetch script answered.
///
/// # Errors
/// [`SourceError::Unauthorized`] for a signed-out browser, [`SourceError::NotFound`] when
/// `SharePoint` has no such item or the person cannot see it, and [`SourceError::Refused`] when
/// Teams holds no token for `SharePoint` yet or `SharePoint` refuses otherwise.
fn fetched(page: &Value) -> Result<FetchedMeeting, SourceError> {
    let page: FetchPage = serde_json::from_value(page.clone()).map_err(|error| {
        SourceError::Refused(format!(
            "Teams sent a recording in an unknown shape: {error}"
        ))
    })?;
    if !page.signed_in {
        return Err(SourceError::Unauthorized(SIGNED_OUT.to_owned()));
    }
    if !page.ok {
        return Err(fetch_error(&page));
    }

    let recording = page.recording.and_then(|download| {
        let url = Url::parse(&download.url).ok().filter(|url| {
            is_public_download(url)
                && url
                    .host_str()
                    .is_some_and(|host| host.ends_with(".sharepoint.com"))
        });
        if url.is_none() {
            event!(
                name: "teams.meetings.download.refused",
                Level::WARN,
                "Teams offered a recording download that is not on SharePoint; it is not kept",
            );
        }
        url.map(|url| RecordingSource {
            url: SecretString::from(String::from(url)),
            kind: if download.is_video {
                RecordingKind::Video
            } else {
                RecordingKind::Audio
            },
        })
    });
    let entries = page.transcript.unwrap_or_default();
    let entry_count = entries.len();
    let transcript: Vec<TranscriptLine> = entries.iter().filter_map(line).collect();
    if transcript.len() < entry_count {
        event!(
            name: "teams.meetings.transcript.entries_skipped",
            Level::DEBUG,
            transcript.skipped = entry_count - transcript.len(),
            "transcript entries without times or text were left out",
        );
    }
    if recording.is_none() && transcript.is_empty() {
        return Err(SourceError::Refused(
            "the recording offers neither a download nor a transcript yet".to_owned(),
        ));
    }
    Ok(FetchedMeeting {
        transcript,
        recording,
    })
}

/// Why a fetch the page refused failed, worded for the person.
fn fetch_error(page: &FetchPage) -> SourceError {
    match (page.error.as_deref(), page.status) {
        (Some("http_error"), Some(401)) => SourceError::Unauthorized(SIGNED_OUT.to_owned()),
        (Some("http_error"), Some(403 | 404)) => SourceError::NotFound,
        (Some("http_error"), Some(status)) => SourceError::Refused(format!(
            "SharePoint answered {status} reading the recording's {}",
            page.step.as_deref().unwrap_or("item")
        )),
        (Some("token_expired"), _) => SourceError::Refused(
            "Teams' SharePoint token has expired; it renews once Teams is used in its browser"
                .to_owned(),
        ),
        (Some("token_not_found"), _) => SourceError::Refused(
            "Teams holds no SharePoint token yet; open a meeting's recording in its browser once"
                .to_owned(),
        ),
        (error, _) => SourceError::Refused(format!(
            "Teams could not read the recording: {}",
            error.unwrap_or("no reason given")
        )),
    }
}

/// One transcript line from an entry, or `None` for one without times or text.
fn line(entry: &PageEntry) -> Option<TranscriptLine> {
    let start_ms = clock_ms(entry.start.as_deref()?)?;
    let end_ms = clock_ms(entry.end.as_deref()?)?.max(start_ms);
    let text = entry.text.trim();
    if text.is_empty() {
        return None;
    }
    Some(TranscriptLine {
        start_ms,
        end_ms,
        speaker: entry.speaker.trim().to_owned(),
        text: text.to_owned(),
    })
}

/// Milliseconds in `H:MM:SS` with an optional decimal fraction of a second: a transcript
/// offset, `00:00:03.3587846` (in ten-millionths), or a recording's length, `0:22:10.06`.
fn clock_ms(clock: &str) -> Option<i64> {
    let mut parts = clock.trim().split(':');
    let (Some(hours), Some(minutes), Some(seconds), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let (whole, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    let hours: i64 = hours.parse().ok()?;
    let minutes: i64 = minutes.parse().ok()?;
    let whole: i64 = whole.parse().ok()?;
    if minutes >= 60 || whole >= 60 || !fraction.chars().all(|digit| digit.is_ascii_digit()) {
        return None;
    }
    // The fraction's first three digits are its milliseconds.
    let milliseconds: String = fraction.chars().chain("000".chars()).take(3).collect();
    let milliseconds: i64 = milliseconds.parse().ok()?;
    let total = Duration::hours(hours)
        + Duration::minutes(minutes)
        + Duration::seconds(whole)
        + Duration::milliseconds(milliseconds);
    Some(total.num_milliseconds())
}

fn truncate(text: &str, max_characters: usize) -> String {
    text.trim().chars().take(max_characters).collect()
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;

    const ITEM: &str = "https://contoso-my.sharepoint.com/_api/v2.1/drives/b!drive/items/01ITEM";

    fn recording(started_at: &str) -> Value {
        json!({
            "item": ITEM,
            "url": "https://contoso-my.sharepoint.com/:v:/g/personal/ada_contoso_com/share",
            "title": "Launch review",
            "startedAt": started_at,
            "duration": "0:22:10.06",
            "organizer": "ada@contoso.com",
            "participants": ["Ada Lovelace", "Grace Hopper"],
            "hasTranscript": true,
        })
    }

    #[test]
    fn a_recording_is_listed_by_its_drive_item_with_its_meeting() {
        let page = json!({
            "signedIn": true,
            "found": true,
            "recordings": [recording("2026-11-10T17:30:15.7903791Z")],
        });
        let meetings = listed(&page, None, 50).expect("a listing");
        assert_eq!(
            meetings,
            [ListedMeeting {
                external_id: ITEM.to_owned(),
                title: "Launch review".to_owned(),
                started_at: "2026-11-10T17:30:15.790379100Z".parse().expect("a time"),
                duration_ms: Some(1_330_060),
                organizer: "ada@contoso.com".to_owned(),
                participants: vec!["Ada Lovelace".to_owned(), "Grace Hopper".to_owned()],
                url: Some(
                    "https://contoso-my.sharepoint.com/:v:/g/personal/ada_contoso_com/share"
                        .to_owned()
                ),
            }]
        );
    }

    #[test]
    fn a_listing_starts_at_since_newest_first_and_stops_at_its_limit() {
        let mut older = recording("2026-11-01T17:00:00Z");
        older["item"] =
            json!("https://contoso-my.sharepoint.com/_api/v2.1/drives/b!drive/items/01OLD");
        let mut oldest = recording("2026-10-01T17:00:00Z");
        oldest["item"] =
            json!("https://contoso-my.sharepoint.com/_api/v2.1/drives/b!drive/items/01OLDEST");
        let page = json!({
            "signedIn": true,
            "found": true,
            "recordings": [older, recording("2026-11-10T17:00:00Z"), oldest],
        });
        let since = "2026-10-15T00:00:00Z".parse().expect("a time");
        let ids: Vec<String> = listed(&page, Some(since), 50)
            .expect("a listing")
            .into_iter()
            .map(|meeting| meeting.external_id)
            .collect();
        assert_eq!(
            ids,
            [
                ITEM,
                "https://contoso-my.sharepoint.com/_api/v2.1/drives/b!drive/items/01OLD"
            ]
        );
        assert_eq!(listed(&page, None, 1).expect("a listing").len(), 1);
    }

    #[test]
    fn a_recording_whose_item_is_not_on_sharepoint_is_not_listed() {
        let mut elsewhere = recording("2026-11-10T17:00:00Z");
        elsewhere["item"] = json!("https://example.org/_api/v2.1/drives/b!drive/items/01ITEM");
        let page = json!({ "signedIn": true, "found": true, "recordings": [elsewhere] });
        assert!(listed(&page, None, 50).expect("a listing").is_empty());
    }

    #[test]
    fn a_signed_out_browser_or_a_missing_store_lists_nothing_and_says_why() {
        let signed_out = json!({ "signedIn": false, "found": false, "recordings": [] });
        assert!(matches!(
            listed(&signed_out, None, 50),
            Err(SourceError::Unauthorized(_))
        ));
        let no_store = json!({ "signedIn": true, "found": false, "recordings": [] });
        assert!(matches!(
            listed(&no_store, None, 50),
            Err(SourceError::Refused(_))
        ));
    }

    #[test]
    fn only_a_sharepoint_drive_item_is_ever_handed_to_the_page() {
        assert!(drive_item(ITEM).is_some());
        for refused in [
            "http://contoso-my.sharepoint.com/_api/v2.1/drives/b!drive/items/01ITEM",
            "https://contoso-my.sharepoint.com.example.org/_api/v2.1/drives/d/items/i",
            "https://contoso-my.sharepoint.com/_api/v2.1/drives/d/items/i/children",
            "https://contoso-my.sharepoint.com/_api/v2.1/drives/d/items/i?select=x",
            "https://contoso-my.sharepoint.com/_api/v2.1/me/drive",
            "01ITEM",
        ] {
            assert!(drive_item(refused).is_none(), "{refused}");
        }
    }

    #[test]
    fn a_fetched_recording_has_its_download_and_its_speakers_lines() {
        let page = json!({
            "signedIn": true,
            "ok": true,
            "recording": {
                "url": "https://contoso-my.sharepoint.com/personal/ada/_layouts/15/download.aspx?UniqueId=1&tempauth=x",
                "mimeType": "video/mp4",
                "isVideo": true,
            },
            "transcript": [
                {
                    "start": "00:00:14.4571439",
                    "end": "00:00:18.2171439",
                    "speaker": "Ada Lovelace",
                    "text": "Are we launching Friday?",
                },
                { "start": "00:01:02.0000000", "end": "00:01:03.5000000", "speaker": "Grace Hopper", "text": " Yes. " },
                { "start": null, "end": "00:01:05.0000000", "speaker": "Grace Hopper", "text": "No start" },
                { "start": "00:01:06.0000000", "end": "00:01:07.0000000", "speaker": "Grace Hopper", "text": "  " },
            ],
        });
        let meeting = fetched(&page).expect("a meeting");
        assert_eq!(
            meeting.transcript,
            [
                TranscriptLine {
                    start_ms: 14_457,
                    end_ms: 18_217,
                    speaker: "Ada Lovelace".to_owned(),
                    text: "Are we launching Friday?".to_owned(),
                },
                TranscriptLine {
                    start_ms: 62_000,
                    end_ms: 63_500,
                    speaker: "Grace Hopper".to_owned(),
                    text: "Yes.".to_owned(),
                },
            ]
        );
        let recording = meeting.recording.expect("a recording");
        assert_eq!(recording.kind, RecordingKind::Video);
        assert!(recording.url.expose_secret().contains("download.aspx"));
    }

    #[test]
    fn a_recording_that_cannot_be_downloaded_is_read_from_its_transcript_alone() {
        let page = json!({
            "signedIn": true,
            "ok": true,
            "recording": null,
            "transcript": [{ "start": "00:00:01.0", "end": "00:00:02.0", "speaker": "Ada", "text": "Hello" }],
        });
        let meeting = fetched(&page).expect("a meeting");
        assert!(meeting.recording.is_none());
        assert_eq!(meeting.transcript.len(), 1);
    }

    #[test]
    fn a_download_off_sharepoint_is_never_kept() {
        let page = json!({
            "signedIn": true,
            "ok": true,
            "recording": { "url": "https://example.org/recording.mp4", "isVideo": true },
            "transcript": [{ "start": "00:00:01.0", "end": "00:00:02.0", "speaker": "Ada", "text": "Hello" }],
        });
        assert!(fetched(&page).expect("a meeting").recording.is_none());
    }

    #[test]
    fn a_refused_fetch_says_why_in_the_terms_the_fetch_step_acts_on() {
        let refusal = |error: &str, status: Option<u16>| {
            fetched(&json!({ "signedIn": true, "ok": false, "error": error, "status": status }))
                .expect_err("a refusal")
        };
        assert!(matches!(
            refusal("http_error", Some(404)),
            SourceError::NotFound
        ));
        assert!(matches!(
            refusal("http_error", Some(403)),
            SourceError::NotFound
        ));
        assert!(matches!(
            refusal("http_error", Some(401)),
            SourceError::Unauthorized(_)
        ));
        assert!(matches!(
            refusal("http_error", Some(503)),
            SourceError::Refused(_)
        ));
        assert!(matches!(
            refusal("token_expired", None),
            SourceError::Refused(_)
        ));
        assert!(matches!(
            refusal("token_not_found", None),
            SourceError::Refused(_)
        ));
        let nothing =
            json!({ "signedIn": true, "ok": true, "recording": null, "transcript": null });
        assert!(matches!(fetched(&nothing), Err(SourceError::Refused(_))));
    }

    #[test]
    fn clocks_read_as_milliseconds() {
        assert_eq!(clock_ms("00:00:03.3587846"), Some(3_358));
        assert_eq!(clock_ms("01:02:03.4"), Some(3_723_400));
        assert_eq!(clock_ms("0:22:10.06"), Some(1_330_060));
        assert_eq!(clock_ms("0:24:54"), Some(1_494_000));
        assert_eq!(clock_ms("00:61:00.0"), None);
        assert_eq!(clock_ms("12.5"), None);
        assert_eq!(clock_ms("00:00:01.x"), None);
    }

    #[test]
    fn the_script_never_reads_what_microsofts_ai_wrote() {
        // Teams keeps its meeting recap in a store of its own and marks meetings that have one;
        // SharePoint serves AI notes beside a recording's transcripts. None of it is read.
        for forbidden in [
            "meeting-recap-manager",
            "isRecapEnabled",
            "/media/notes",
            "/media/recap",
        ] {
            assert!(!SCRIPT.contains(forbidden), "the script reads {forbidden}");
        }
    }
}
