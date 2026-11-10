// Copyright © 2026 Jalapeno Labs

//! Slack huddles as meetings: each huddle Slack transcribed, read as its person through the
//! browser their Slack connector is signed in to. See `docs/meetings.md`, Slack huddles.
//!
//! # How it reads
//!
//! Through the Slack connector's page script (`slack.js`, through [`Slack::ask`]), with the web
//! client's own methods, each one evaluation in the signed-in tab:
//!
//! 1. `huddles.history` lists the person's huddles in each workspace, newest first, 50 a page.
//!    A huddle that ended and has a `transcript_file_id` is a meeting.
//! 2. `huddles.get { huddle_id }` names that file again when the meeting is fetched.
//! 3. `files.info { file, include_transcription }` answers a huddle transcript's lines
//!    (`huddle_transcription.lines`: who spoke, from the huddle's start, and what they said).
//!    Where the workspace keeps AI notes, the huddle's file is the notes canvas instead, whose
//!    metadata names the transcript file (`huddle_transcript_file_id`), which is read in turn.
//! 4. `users.info` and `conversations.info` name the speakers, participants, and channel.
//!
//! Nothing of Slack's AI notes is read: the canvas is only ever asked for its metadata, to
//! find the transcript beside it. A huddle keeps no recording, so each is a meeting in words
//! alone and needs no storage location. These are the web client's methods, not Slack's
//! published API, observed in Slack's current web client.
//!
//! # What it reads
//!
//! The huddles of the workspaces and conversations its connector's content picks, as its
//! messages are. While the "Read meeting recordings and transcripts" toggle is off
//! ([`super::Content::read_meetings`]) the meetings loop does not list it at all
//! (`crate::meetings::sources::Source::reads`).

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use super::{Call, Content, ConversationKind, SIGNED_OUT_ERRORS, Slack, State, Tab, Team};
use crate::connectors::SignInState;
use crate::meetings::source::{
    FetchedMeeting, ListedMeeting, MeetingSource, SourceError, TranscriptLine,
};
use crate::models::browser_session::BrowserSession;

/// How many huddles one page of `huddles.history` asks for; Slack answers 50 when asked.
const HUDDLES_PER_PAGE: u32 = 50;
/// The most pages one listing reads per workspace. Listings come every ten minutes and start
/// where the last left off, so this only bounds a new connector's first one.
const PAGES_PER_LISTING: usize = 4;
/// The most names one listing or fetch looks up: a huddle's participants and its channel.
const LOOKUPS_PER_ASK: usize = 60;

/// Slack's errors for a huddle or file that is gone, or that this person cannot see.
const MISSING_ERRORS: &[&str] = &[
    "file_not_found",
    "file_deleted",
    "huddle_not_found",
    "not_found",
    "channel_not_found",
];

/// Slack's huddles, read through the Slack connector's browser.
#[derive(Debug, Clone)]
pub struct SlackMeetings {
    slack: Slack,
}

/// What a Slack connector reads its huddles as: its browser, and its content, whose picks bound
/// which huddles are read.
#[derive(Debug)]
pub struct SlackSignIn {
    pub browser: BrowserSession,
    pub content: Content,
}

/// One huddle as `huddles.history` or `huddles.get` answers it, when it ended with a transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Huddle {
    pub team: String,
    pub id: String,
    /// The conversation it was held in.
    pub channel: Option<String>,
    pub kind: ConversationKind,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub created_by: Option<String>,
    /// Everyone who joined, in the order Slack lists them.
    pub participants: Vec<String>,
    /// The thread its conversation shows it in, which its link opens.
    pub thread_root_ts: Option<String>,
    /// The file Slack keeps its transcript in, or the AI notes canvas beside it.
    pub transcript_file_id: String,
}

impl SlackMeetings {
    pub const fn new(slack: Slack) -> Self {
        Self { slack }
    }

    /// Asks the page for `calls`, answering the workspaces it is signed in to and the results.
    async fn ask(
        &self,
        sign_in: &SlackSignIn,
        calls: &[Call],
    ) -> Result<(Vec<Team>, Vec<Value>), SourceError> {
        match self.slack.ask(&sign_in.browser, false, calls).await {
            Ok(Tab::Open { teams, results }) => Ok((teams, results)),
            Ok(Tab::Missing(SignInState::SignedOut)) => Err(SourceError::Unauthorized(
                "Slack is signed out in the connector's browser: sign in again on its page"
                    .to_owned(),
            )),
            Ok(Tab::Missing(_unknown)) => Err(SourceError::Refused(
                "Slack is not open in the connector's browser yet".to_owned(),
            )),
            Err(error) => Err(SourceError::Refused(error.to_string())),
        }
    }

    /// `team`'s huddles that ended at or after `since`, page by page, newest first.
    async fn history(
        &self,
        sign_in: &SlackSignIn,
        team: &str,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<Huddle>, SourceError> {
        let mut huddles = Vec::new();
        let mut cursor: Option<String> = None;
        for _page in 0..PAGES_PER_LISTING {
            let mut params = json!({ "limit": HUDDLES_PER_PAGE });
            if let Some(cursor) = &cursor {
                params["cursor"] = json!(cursor);
            }
            let call = Call::new(team, "huddles.history", params);
            let (_teams, results) = self.ask(sign_in, &[call]).await?;
            let answer = results.into_iter().next().unwrap_or(Value::Null);
            answered("huddles.history", &answer)?;
            let page = answer["huddles"].as_array().cloned().unwrap_or_default();
            // Slack lists the newest first, so a page whose last huddle ended before `since`
            // is the last one this listing needs.
            let oldest_end = page.last().and_then(|entry| seconds(&entry["date_end"]));
            huddles.extend(page.iter().filter_map(|entry| huddle_of(team, entry)));
            cursor = answer["response_metadata"]["next_cursor"]
                .as_str()
                .filter(|next| !next.is_empty())
                .map(str::to_owned);
            let past_since = since.is_some_and(|since| oldest_end.is_some_and(|end| end < since));
            if page.is_empty() || cursor.is_none() || past_since {
                break;
            }
        }
        Ok(huddles)
    }

    /// The names of `users` and `channels` of `team`, by `team/id`, as the reader keeps them.
    async fn names(
        &self,
        sign_in: &SlackSignIn,
        team: &str,
        users: &BTreeSet<&str>,
        channels: &BTreeSet<&str>,
    ) -> Result<State, SourceError> {
        let mut lookups: Vec<Call> = Vec::new();
        for user in users {
            lookups.push(Call::new(team, "users.info", json!({ "user": user })));
        }
        for channel in channels {
            lookups.push(Call::new(
                team,
                "conversations.info",
                json!({ "channel": channel }),
            ));
        }
        lookups.truncate(LOOKUPS_PER_ASK);
        let mut names = State::default();
        if lookups.is_empty() {
            return Ok(names);
        }
        let (_teams, results) = self.ask(sign_in, &lookups).await?;
        super::remember_names(&lookups, &results, &mut names);
        Ok(names)
    }

    /// One file's `files.info`, with its transcript when it is one.
    async fn file(
        &self,
        sign_in: &SlackSignIn,
        team: &str,
        file: &str,
    ) -> Result<Value, SourceError> {
        let call = Call::new(
            team,
            "files.info",
            json!({ "file": file, "include_transcription": true }),
        );
        let (_teams, results) = self.ask(sign_in, &[call]).await?;
        let answer = results.into_iter().next().unwrap_or(Value::Null);
        answered("files.info", &answer)?;
        Ok(answer["file"].clone())
    }
}

impl MeetingSource for SlackMeetings {
    /// The Slack connector's browser, and its content.
    type SignIn = SlackSignIn;

    async fn list(
        &self,
        sign_in: &SlackSignIn,
        since: Option<DateTime<Utc>>,
        limit: u32,
    ) -> Result<Vec<ListedMeeting>, SourceError> {
        let content = &sign_in.content;
        let (teams, _results) = self.ask(sign_in, &[]).await?;
        let mut huddles = Vec::new();
        for team in teams.iter().filter(|team| content.reads_team(&team.id)) {
            huddles.extend(self.history(sign_in, &team.id, since).await?);
        }
        let chosen = chosen(huddles, content, since, limit);

        let mut listed = Vec::with_capacity(chosen.len());
        for team in &teams {
            let of_team: Vec<&Huddle> = chosen
                .iter()
                .filter(|huddle| huddle.team == team.id)
                .collect();
            if of_team.is_empty() {
                continue;
            }
            let mut users = BTreeSet::new();
            let mut channels = BTreeSet::new();
            for huddle in &of_team {
                users.extend(huddle.created_by.as_deref());
                users.extend(huddle.participants.iter().map(String::as_str));
                if huddle.kind == ConversationKind::Channel {
                    channels.extend(huddle.channel.as_deref());
                }
            }
            let names = self.names(sign_in, &team.id, &users, &channels).await?;
            listed.extend(
                of_team
                    .into_iter()
                    .map(|huddle| listed_meeting(huddle, team, &names)),
            );
        }
        listed.sort_by_key(|meeting| std::cmp::Reverse(meeting.started_at));
        Ok(listed)
    }

    async fn fetch(
        &self,
        sign_in: &SlackSignIn,
        external_id: &str,
    ) -> Result<FetchedMeeting, SourceError> {
        let Some((team, huddle_id)) = external_id.split_once('/') else {
            return Err(SourceError::NotFound);
        };
        let call = Call::new(team, "huddles.get", json!({ "huddle_id": huddle_id }));
        let (_teams, results) = self.ask(sign_in, &[call]).await?;
        let answer = results.into_iter().next().unwrap_or(Value::Null);
        answered("huddles.get", &answer)?;
        let Some(huddle) = huddle_of(team, &answer["huddle"]) else {
            return Err(SourceError::NotFound);
        };

        let mut file = self.file(sign_in, team, &huddle.transcript_file_id).await?;
        if file["huddle_transcription"].is_null() {
            // The AI notes canvas: only its metadata, which names the transcript beside it.
            let Some(transcript) = file["huddle_transcript_file_id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
            else {
                return Err(SourceError::NotFound);
            };
            file = self.file(sign_in, team, &transcript).await?;
        }
        let transcription = &file["huddle_transcription"];
        if transcription.is_null() {
            return Err(SourceError::NotFound);
        }

        let speakers: BTreeSet<&str> = transcription["lines"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|line| line["user_id"].as_str())
            .collect();
        let names = self
            .names(sign_in, team, &speakers, &BTreeSet::new())
            .await?;
        Ok(FetchedMeeting {
            transcript: transcript_of(team, transcription, &names),
            recording: None,
        })
    }
}

/// Slack's error, as the source error it is, when `answer` is not `ok`.
fn answered(method: &str, answer: &Value) -> Result<(), SourceError> {
    if answer["ok"] == true {
        return Ok(());
    }
    let error = answer["error"].as_str().unwrap_or("unknown");
    if SIGNED_OUT_ERRORS.contains(&error) || error == "team_not_signed_in" {
        return Err(SourceError::Unauthorized(format!(
            "Slack refused {method} with {error}: sign in again in the connector's browser"
        )));
    }
    if MISSING_ERRORS.contains(&error) {
        return Err(SourceError::NotFound);
    }
    Err(SourceError::Refused(format!(
        "Slack answered {method} with {error}"
    )))
}

/// A Unix time in whole seconds, as Slack's huddles carry them.
fn seconds(value: &Value) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(value.as_i64()?, 0)
}

/// `entry` as a huddle, when it ended and has a transcript: one still going, or one Slack did
/// not transcribe, is not a meeting yet or at all.
pub fn huddle_of(team: &str, entry: &Value) -> Option<Huddle> {
    let id = entry["id"].as_str().filter(|id| !id.is_empty())?;
    let transcript_file_id = entry["transcript_file_id"]
        .as_str()
        .filter(|file| !file.is_empty())?;
    if entry["has_ended"] != true {
        return None;
    }
    let started_at = seconds(&entry["date_start"])?;
    let ended_at = seconds(&entry["date_end"])?.max(started_at);
    let channel = entry["channels"][0].as_str().map(str::to_owned);
    // Slack names the conversation's type; without it, a direct message's id starts with `D`.
    let is_direct_id = channel.as_deref().is_some_and(|id| id.starts_with('D'));
    let kind = match entry["channel_type"].as_str() {
        Some("im") => ConversationKind::Direct,
        Some("mpim") => ConversationKind::Group,
        None if is_direct_id => ConversationKind::Direct,
        Some(_) | None => ConversationKind::Channel,
    };
    Some(Huddle {
        team: team.to_owned(),
        id: id.to_owned(),
        channel,
        kind,
        started_at,
        ended_at,
        created_by: entry["created_by"].as_str().map(str::to_owned),
        participants: entry["participant_history"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        thread_root_ts: entry["thread_root_ts"].as_str().map(str::to_owned),
        transcript_file_id: transcript_file_id.to_owned(),
    })
}

/// The huddles a listing answers: those the content reads that ended at or after `since`,
/// newest first, at most `limit`. Ending, not starting, decides, since the loop's cursor is
/// the newest start it saw: a long huddle that started before a shorter one and ended after it
/// is still listed once its transcript is there.
pub fn chosen(
    huddles: Vec<Huddle>,
    content: &Content,
    since: Option<DateTime<Utc>>,
    limit: u32,
) -> Vec<Huddle> {
    let mut chosen: Vec<Huddle> = huddles
        .into_iter()
        .filter(|huddle| since.is_none_or(|since| huddle.ended_at >= since))
        .filter(|huddle| match &huddle.channel {
            Some(channel) => content.reads(&huddle.team, channel, huddle.kind),
            None => content.reads_team(&huddle.team),
        })
        .collect();
    chosen.sort_by_key(|huddle| std::cmp::Reverse(huddle.started_at));
    chosen.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    chosen
}

/// `huddle` as a listing shows it, named with `names` (by `team/id`).
pub fn listed_meeting(huddle: &Huddle, team: &Team, names: &State) -> ListedMeeting {
    let name_of = |id: &str| {
        names
            .names
            .get(&format!("{}/{id}", huddle.team))
            .cloned()
            .unwrap_or_else(|| id.to_owned())
    };
    let participants: Vec<String> = huddle
        .participants
        .iter()
        .map(|user| name_of(user))
        .collect();
    let title = match (huddle.kind, &huddle.channel) {
        (ConversationKind::Channel, Some(channel)) => format!("Huddle in #{}", name_of(channel)),
        _ if participants.is_empty() => "Huddle".to_owned(),
        _ => format!("Huddle with {}", participants.join(", ")),
    };
    let url = huddle
        .channel
        .as_deref()
        .zip(huddle.thread_root_ts.as_deref())
        .and_then(|(channel, root)| super::permalink(&team.url, channel, root, None));
    ListedMeeting {
        external_id: format!("{}/{}", huddle.team, huddle.id),
        title,
        started_at: huddle.started_at,
        duration_ms: Some((huddle.ended_at - huddle.started_at).num_milliseconds()),
        organizer: huddle
            .created_by
            .as_deref()
            .map(name_of)
            .unwrap_or_default(),
        participants,
        url,
    }
}

/// A huddle transcript's lines in order. Slack times each line's start from the huddle's start
/// and gives no end, so a line ends where the next begins, and the last where the huddle ended.
pub fn transcript_of(team: &str, transcription: &Value, names: &State) -> Vec<TranscriptLine> {
    let length_ms = match (
        transcription["date_start"].as_i64(),
        transcription["date_end"].as_i64(),
    ) {
        (Some(start), Some(end)) => (end - start).saturating_mul(1000).max(0),
        _ => 0,
    };
    let mut spoken: Vec<(i64, String, String)> = transcription["lines"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|line| {
            let start_ms = line["start_time_ms"].as_i64()?.max(0);
            let text = line["contents"].as_str()?.trim();
            if text.is_empty() {
                return None;
            }
            let speaker = line["user_id"]
                .as_str()
                .map(|user| {
                    names
                        .names
                        .get(&format!("{team}/{user}"))
                        .cloned()
                        .unwrap_or_else(|| user.to_owned())
                })
                .unwrap_or_default();
            Some((start_ms, speaker, text.to_owned()))
        })
        .collect();
    spoken.sort_by_key(|(start_ms, _speaker, _text)| *start_ms);

    let next_starts: Vec<i64> = spoken
        .iter()
        .skip(1)
        .map(|(start_ms, _speaker, _text)| *start_ms)
        .collect();
    spoken
        .into_iter()
        .enumerate()
        .map(|(index, (start_ms, speaker, text))| {
            let end_ms = next_starts.get(index).copied().unwrap_or(length_ms);
            TranscriptLine {
                start_ms,
                end_ms: end_ms.max(start_ms),
                speaker,
                text,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
