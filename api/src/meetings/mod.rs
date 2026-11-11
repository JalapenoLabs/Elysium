// Copyright © 2026 Jalapeno Labs

//! Meetings: recordings and transcripts read from a meeting source as their person, cut into
//! topic segments that land on the timeline. See `docs/meetings.md`.
//!
//! A meeting is the one event that happened; each of its captures is one record of it, as its
//! source gave it (`models::meeting_capture`). Its final transcript is a stream of words with
//! stable ids ([`final_transcript`], `models::meeting_word`), every mark on it a range of those
//! words, and every change to it is in its history ([`history`]).
//!
//! # The loop
//!
//! [`Meetings::start`] runs until shutdown:
//!
//! - Every [`POLL_INTERVAL`] it lists each active meeting connector's new meetings
//!   ([`source::MeetingSource::list`]) and stores each one it has not seen as a `pending`
//!   meeting with its first capture. The connector's cursor remembers the newest start time it
//!   saw.
//! - Every [`PROCESS_INTERVAL`] it starts the runs of the meeting workflow ([`workflow`]) that
//!   unfinished meetings need: one from the fetch for a meeting that never had one, and one from
//!   segmenting for a meeting fetched before segmenting was a step of its run
//!   ([`workflow::run_to_start`]). Every step of a meeting is a step of its run: fetching
//!   ([`fetch`]), transcribing ([`transcribe`]), correcting ([`correct`]), and segmenting and
//!   publishing ([`segment`]), each on the engine's step queue with its own attempts, so the
//!   loop follows nothing itself.
//!
//! A meeting connector is one of any kind that provides meetings
//! (`kinds::providing(Provides::Meetings)`), and each is read through its kind's source
//! ([`sources::Sources::for_kind`]), so nothing here names a source.
//!
//! # What is published
//!
//! `meeting.changed` carries a meeting's id, state, and owner, never its title or transcript:
//! every client of the event stream hears it. Each segment's record publishes
//! `timelineRecord.created`, which is where Elysia's intake hears of it.

pub mod align;
pub mod alignment;
pub mod clock;
pub mod conversation;
pub mod correct;
pub mod correction;
pub mod duplicates;
pub mod fetch;
pub mod final_transcript;
pub mod fireflies;
pub mod fireflies_browser;
pub mod fixes;
pub mod history;
pub mod media;
pub mod segment;
pub mod segmentation;
pub mod source;
pub mod sources;
pub mod transcribe;
pub mod workflow;

#[cfg(test)]
mod same_meeting_tests;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use self::source::{ListedMeeting, SourceError};
use self::sources::Sources;
use self::workflow::RunToStart;
use crate::connectors::activity::Activity;
use crate::connectors::kinds::{self, Provides};
use crate::connectors::{ConnectorKind, SignInState};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::meeting_uploads::{self, staging::Staging};
use crate::models::connector::{self, Connector, PollOutcome};
use crate::models::connector_run::ConnectorRun;
use crate::models::meeting::{self, Meeting, NewMeeting};
use crate::models::meeting_capture::{self, MeetingCapture, NewMeetingCapture};
use crate::models::meeting_upload;
use crate::models::storage_location::{self, StorageLocation};
use crate::models::workflow_run::{self, WorkflowRun};
use crate::realtime::{EventBus, ServerEvent};
use crate::workflows;

/// How often each connector's new meetings are listed. A meeting is useful on the timeline
/// within minutes of ending, not seconds, and the strictest source's smallest plan
/// (Fireflies', 500 calls a day) allows it: ten minutes uses 144 calls.
pub const POLL_INTERVAL: Duration = Duration::from_mins(10);

/// The most meetings one listing asks for. A source that allows fewer asks for what it allows,
/// and the cursor carries the rest to the next listing.
const LISTING_SIZE: u32 = 50;

/// How often unfinished meetings get the runs they need.
pub const PROCESS_INTERVAL: Duration = Duration::from_mins(1);

/// The most finished uploads one pass removes the parts of, so a backlog clears over a few passes
/// rather than holding one.
const UPLOAD_SWEEP_BATCH: i64 = 20;

/// How far back an account's first listing reaches, so a newly connected account brings its
/// recent meetings with it, but not its whole history.
const FIRST_LISTING_DAYS: i64 = 14;

/// What a segment's timeline record is.
pub const SEGMENT_SOURCE_KIND: &str = "meeting";

/// The content field every meeting source declares, a toggle: "Wait for my approval before
/// acting on meetings", off until its person turns it on. See `correct`, Waiting for approval.
pub const WAIT_FOR_APPROVAL_FIELD: &str = "waitForApproval";

/// Whether meeting source `connector` waits for its person's approval of each meeting's
/// corrected transcript before the meeting is acted on. A connector that never saved the
/// field, such as one made before it existed, does not wait.
pub fn waits_for_approval(connector: &Connector) -> bool {
    connector
        .content
        .get(WAIT_FOR_APPROVAL_FIELD)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

#[derive(Clone)]
pub struct Meetings {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    events: EventBus,
    activity: Activity,
    cipher: Arc<Cipher>,
    sources: Sources,
    staging: Staging,
}

impl std::fmt::Debug for Meetings {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Meetings").finish_non_exhaustive()
    }
}

/// What the meetings service is built from.
#[derive(Clone)]
pub struct MeetingsDeps {
    pub database: Pool,
    pub events: EventBus,
    pub activity: Activity,
    pub cipher: Arc<Cipher>,
    pub sources: Sources,
    /// Where uploads' parts wait, which the loop clears of uploads abandoned for
    /// `meeting_uploads::STALE_AFTER`.
    pub staging: Staging,
}

impl Meetings {
    pub fn new(deps: MeetingsDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                database: deps.database,
                events: deps.events,
                activity: deps.activity,
                cipher: deps.cipher,
                sources: deps.sources,
                staging: deps.staging,
            }),
        }
    }

    /// Lists one connector's new meetings now and starts the runs unfinished meetings need,
    /// for a sync someone asked for.
    pub async fn sync(&self, connector: &Connector) {
        if let Err(error) = self.poll_account(connector).await {
            event!(
                name: "meetings.sync.failed",
                Level::WARN,
                connector.id = %connector.id,
                error.message = %format!("{error:#}"),
                "cannot record a meeting connector's listing",
            );
        }
        self.process().await;
    }

    /// Lists and processes meetings until `shutdown` is cancelled. A pass under way when it
    /// is cancelled stops where it is; every step is safe to repeat.
    pub fn start(&self, shutdown: CancellationToken) -> JoinHandle<()> {
        let meetings = self.clone();
        tokio::spawn(async move {
            let mut polls = tokio::time::interval(POLL_INTERVAL);
            polls.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut passes = tokio::time::interval(PROCESS_INTERVAL);
            passes.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    () = shutdown.cancelled() => return,
                    _tick = polls.tick() => {
                        tokio::select! {
                            () = shutdown.cancelled() => return,
                            () = meetings.poll() => {}
                        }
                    }
                    _tick = passes.tick() => {
                        tokio::select! {
                            () = shutdown.cancelled() => return,
                            () = meetings.process() => {}
                        }
                    }
                }
            }
        })
    }

    async fn connection(
        &self,
    ) -> anyhow::Result<diesel_async::pooled_connection::deadpool::Object<AsyncPgConnection>> {
        self.inner
            .database
            .get()
            .await
            .context("no database connection available")
    }

    /// Lists every active meeting connector's new meetings.
    async fn poll(&self) {
        let kinds: Vec<&str> = kinds::providing(Provides::Meetings)
            .into_iter()
            .map(ConnectorKind::as_str)
            .collect();
        let accounts = match self.connection().await {
            Ok(mut connection) => connector::list_active(&mut connection, &kinds)
                .await
                .map_err(anyhow::Error::from),
            Err(error) => Err(error),
        };
        let accounts = match accounts {
            Ok(accounts) => accounts,
            Err(error) => {
                event!(
                    name: "meetings.poll.failed",
                    Level::WARN,
                    error.message = %format!("{error:#}"),
                    "cannot list the meeting connectors to read",
                );
                return;
            }
        };
        for account in accounts {
            if let Err(error) = self.poll_account(&account).await {
                event!(
                    name: "meetings.account.failed",
                    Level::WARN,
                    connector.id = %account.id,
                    error.message = %format!("{error:#}"),
                    "cannot record a meeting connector's listing",
                );
            }
        }
    }

    /// Lists one connector's new meetings, stores them, and records how the listing went. A
    /// connector that reads no meetings now, such as a Teams connector with them turned off, is
    /// left as it is.
    async fn poll_account(&self, account: &Connector) -> anyhow::Result<()> {
        if !self.inner.sources.reads_meetings(account) {
            event!(
                name: "meetings.account.not_reading",
                Level::DEBUG,
                connector.id = %account.id,
                "a meeting connector that reads no meetings now was not listed",
            );
            return Ok(());
        }
        let source = {
            let mut connection = self.connection().await?;
            self.inner
                .sources
                .signed_in(&mut connection, &self.inner.cipher, account)
                .await?
        };
        let since = listing_since(account);
        let started = Utc::now();
        let mut run = None;

        let outcome = match source.list(Some(since), LISTING_SIZE).await {
            Ok(listed) => {
                let (newest, stored) = self.store_listed(account, &listed).await?;
                if stored > 0 {
                    run = Some(ConnectorRun::succeeded(account.id, started, stored));
                }
                PollOutcome {
                    sign_in_state: SignInState::SignedIn.as_str().to_owned(),
                    cursor: next_cursor(&account.cursor, newest),
                    last_polled_at: Some(Utc::now()),
                    last_error: None,
                }
            }
            Err(error) => {
                run = Some(ConnectorRun::failed(
                    account.id,
                    started,
                    &error.to_string(),
                ));
                PollOutcome {
                    sign_in_state: match error {
                        SourceError::Unauthorized(_) => SignInState::SignedOut,
                        SourceError::NotFound | SourceError::Refused(_) => SignInState::Unknown,
                    }
                    .as_str()
                    .to_owned(),
                    // Nothing of the cursor changes; it is merged, not replaced.
                    cursor: json!({}),
                    last_polled_at: Some(Utc::now()),
                    last_error: Some(error.to_string()),
                }
            }
        };

        let mut connection = self.connection().await?;
        let updated = connector::record_poll(&mut connection, account.id, outcome).await?;
        drop(connection);
        let changed = updated.sign_in_state != account.sign_in_state
            || updated.last_error != account.last_error;
        match run {
            // Recording a run announces the change too.
            Some(run) => self.inner.activity.record(run).await,
            None if changed => self.inner.activity.changed(account.id),
            None => {}
        }
        Ok(())
    }

    /// Stores each listed meeting the account has not read, and answers the newest start time
    /// the listing held and how many meetings were new.
    async fn store_listed(
        &self,
        account: &Connector,
        listed: &[ListedMeeting],
    ) -> anyhow::Result<(Option<DateTime<Utc>>, usize)> {
        let mut connection = self.connection().await?;
        let external_ids: Vec<String> = listed
            .iter()
            .map(|meeting| meeting.external_id.clone())
            .collect();
        let known =
            meeting_capture::known_external_ids(&mut connection, account.id, &external_ids).await?;
        let kind = ConnectorKind::parse(&account.kind)
            .with_context(|| format!("unknown connector kind {:?}", account.kind))?;

        let mut newest: Option<DateTime<Utc>> = None;
        let mut stored_count = 0;
        for listed_meeting in listed {
            newest = newest.max(Some(listed_meeting.started_at));
            if known.contains(&listed_meeting.external_id) {
                continue;
            }
            let (listed_as, captured_as) = new_meeting(account, kind, listed_meeting);
            let stored = meeting::insert_new(&mut connection, listed_as, captured_as).await?;
            if let Some((stored, _capture)) = stored {
                stored_count += 1;
                self.publish(stored);
            }
        }
        Ok((newest, stored_count))
    }

    /// Starts the run each unfinished meeting needs (`workflow::run_to_start`), and removes what
    /// uploads left in the workspace that nothing needs any more: the uploads abandoned since the
    /// last pass, and the parts of finished ones. One meeting whose run cannot be started is logged
    /// and left for the next pass; the rest go on.
    async fn process(&self) {
        let week_ago = Utc::now() - meeting_uploads::STALE_AFTER;
        self.sweep_stale_uploads(week_ago).await;
        self.sweep_finished_uploads(week_ago).await;
        let needed = match self.runs_needed().await {
            Ok(needed) => needed,
            Err(error) => {
                event!(
                    name: "meetings.process.failed",
                    Level::WARN,
                    error.message = %format!("{error:#}"),
                    "cannot read which meetings need a run",
                );
                return;
            }
        };
        for (unfinished, start) in needed {
            let meeting_id = unfinished.id;
            if let Err(error) = self.start_run(unfinished, start).await {
                event!(
                    name: "meetings.run.unstarted",
                    Level::WARN,
                    meeting.id = %meeting_id,
                    error.message = %format!("{error:#}"),
                    "cannot start a meeting's run; the next pass tries again",
                );
            }
        }
    }

    /// Removes every unfinished upload no part reached since `before`, and its parts. The rows go
    /// first, so nothing can resume an upload whose parts are going, and a part arriving meanwhile
    /// takes them itself (`routes::v1::meeting_uploads::upload_part`); parts that could not be
    /// removed only cost the workspace space, and are logged.
    async fn sweep_stale_uploads(&self, before: DateTime<Utc>) {
        let swept = match self.connection().await {
            Ok(mut connection) => meeting_upload::delete_stale(&mut connection, before)
                .await
                .map_err(anyhow::Error::from),
            Err(error) => Err(error),
        };
        let swept = match swept {
            Ok(swept) => swept,
            Err(error) => {
                event!(
                    name: "meetings.uploads.sweep_failed",
                    Level::WARN,
                    error.message = %format!("{error:#}"),
                    "cannot remove abandoned uploads; the next pass tries again",
                );
                return;
            }
        };
        for upload_id in swept {
            if let Err(error) = self.inner.staging.remove(upload_id).await {
                event!(
                    name: "meetings.uploads.parts_unremoved",
                    Level::WARN,
                    meeting_upload.id = %upload_id,
                    error.message = %error,
                    "an abandoned upload's parts could not be removed from the workspace",
                );
            } else {
                event!(
                    name: "meetings.uploads.swept",
                    Level::INFO,
                    meeting_upload.id = %upload_id,
                    "removed an upload nothing reached for a week",
                );
            }
        }
    }

    /// Removes the parts of finished uploads that no longer need them: each whose capture was
    /// stored, whose fetch could not remove them, and each finished before `before` whose meeting's
    /// run settled without storing it, such as one whose fetch a person skipped. A fetch that failed
    /// and waits for its retry keeps its parts. Each removal is recorded, so it happens once.
    async fn sweep_finished_uploads(&self, before: DateTime<Utc>) {
        let finished = match self.connection().await {
            Ok(mut connection) => {
                meeting_upload::finished_with_parts(&mut connection, before, UPLOAD_SWEEP_BATCH)
                    .await
                    .map_err(anyhow::Error::from)
            }
            Err(error) => Err(error),
        };
        let finished = match finished {
            Ok(finished) => finished,
            Err(error) => {
                event!(
                    name: "meetings.uploads.sweep_failed",
                    Level::WARN,
                    error.message = %format!("{error:#}"),
                    "cannot read which finished uploads keep parts; the next pass tries again",
                );
                return;
            }
        };
        for upload_id in finished {
            let removed = match self.inner.staging.remove(upload_id).await {
                Ok(()) => match self.connection().await {
                    Ok(mut connection) => {
                        meeting_upload::record_parts_removed(&mut connection, upload_id)
                            .await
                            .map_err(anyhow::Error::from)
                    }
                    Err(error) => Err(error),
                },
                Err(error) => Err(anyhow::Error::from(error)),
            };
            if let Err(error) = removed {
                event!(
                    name: "meetings.uploads.parts_unremoved",
                    Level::WARN,
                    meeting_upload.id = %upload_id,
                    error.message = %format!("{error:#}"),
                    "a finished upload's parts could not be removed; the next pass tries again",
                );
            }
        }
    }

    /// Every unfinished meeting that needs a run, with the run it needs, read in one query each
    /// for the meetings, their newest runs, and which of those segment.
    async fn runs_needed(&self) -> anyhow::Result<Vec<(Meeting, RunToStart)>> {
        let mut connection = self.connection().await?;
        let unfinished = meeting::list_unfinished(&mut connection).await?;
        if unfinished.is_empty() {
            return Ok(Vec::new());
        }
        let source_ids: Vec<String> = unfinished
            .iter()
            .map(|unfinished_meeting| unfinished_meeting.id.to_string())
            .collect();
        let newest: HashMap<String, WorkflowRun> = workflow_run::newest_each(
            &mut connection,
            workflow::workflow().name(),
            workflow::SOURCE_KIND,
            &source_ids,
        )
        .await?
        .into_iter()
        .map(|run| (run.source_id.clone(), run))
        .collect();
        let runs: Vec<&WorkflowRun> = newest.values().collect();
        let segmenting = workflow::segmenting_runs(&mut connection, &runs).await?;

        let mut needed = Vec::new();
        for unfinished_meeting in unfinished {
            let run = newest.get(&unfinished_meeting.id.to_string());
            let start = workflow::run_to_start(
                unfinished_meeting.state(),
                run.map(|run| run.state),
                run.is_some_and(|run| segmenting.contains(&run.id)),
            );
            if let Some(start) = start {
                needed.push((unfinished_meeting, start));
            }
        }
        Ok(needed)
    }

    /// Starts `unfinished`'s run as `start` says, and announces the meeting so its pages follow
    /// the new run.
    async fn start_run(&self, unfinished: Meeting, start: RunToStart) -> anyhow::Result<()> {
        let mut connection = self.connection().await?;
        let new_run = match start {
            RunToStart::FromFetch => workflow::run_for(unfinished.id),
            RunToStart::FromSegmenting => {
                workflow::run_from_segmenting(&mut connection, &unfinished).await?
            }
        };
        let started = workflows::start(&mut connection, &self.inner.events, new_run).await?;
        drop(connection);
        if started.created {
            event!(
                name: "meetings.run.started",
                Level::INFO,
                meeting.id = %unfinished.id,
                workflow_run.id = %started.settled.run.id,
                meeting.state = unfinished.state().as_str(),
                "a meeting's run started from where its processing got",
            );
            self.publish(unfinished);
        }
        Ok(())
    }

    fn publish(&self, changed: Meeting) {
        self.inner.events.publish(&ServerEvent::MeetingChanged {
            id: changed.id,
            state: changed.state,
            created_by: changed.created_by,
            entry_id: None,
        });
    }
}

/// The capture a step's input names (`capture_id`), or, for what a run started before meetings
/// had captures was handed, which names the meeting alone, the meeting's first capture. A capture
/// of another meeting reads as none.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when the meeting has no such capture.
pub async fn capture_of(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    capture_id: Option<Uuid>,
) -> diesel::QueryResult<MeetingCapture> {
    let Some(capture_id) = capture_id else {
        event!(
            name: "meetings.capture.unnamed",
            Level::DEBUG,
            meeting.id = %meeting_id,
            "a step's input names no capture, as a run's from before captures does; it reads the \
             meeting's first capture",
        );
        return meeting_capture::first_of(connection, meeting_id).await;
    };
    let capture = meeting_capture::find(connection, capture_id).await?;
    if capture.meeting_id != meeting_id {
        return Err(diesel::result::Error::NotFound);
    }
    Ok(capture)
}

/// What a capture's source, its `source_kind`, is called in English, as the correction's prompt and
/// the Transcription doc name the transcript that heard something: its connector's product, or the
/// kind as stored, capitalized, for a source no connector reads, such as an upload.
pub fn source_name(source_kind: &str) -> String {
    let Some(kind) = ConnectorKind::parse(source_kind) else {
        let mut characters = source_kind.chars();
        return characters.next().map_or_else(String::new, |first| {
            first.to_uppercase().chain(characters).collect()
        });
    };
    let name = match kind {
        ConnectorKind::Jira => "Jira",
        ConnectorKind::Github => "GitHub",
        ConnectorKind::Outlook => "Outlook",
        ConnectorKind::Gmail => "Gmail",
        ConnectorKind::Teams => "Teams",
        ConnectorKind::Slack => "Slack",
        ConnectorKind::Fireflies | ConnectorKind::FirefliesBrowser => "Fireflies",
        ConnectorKind::Confluence => "Confluence",
        ConnectorKind::Youtrack => "YouTrack",
    };
    name.to_owned()
}

/// Storage location `storage_location_id`, with the key that opens it.
async fn location(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    storage_location_id: Uuid,
) -> anyhow::Result<(StorageLocation, secrecy::SecretString)> {
    let location = storage_location::find(connection, storage_location_id).await?;
    let access_key = location
        .access_key(cipher)
        .context("the storage location's access key cannot be decrypted")?;
    Ok((location, access_key))
}

/// Where an account's next listing starts: its cursor's newest start time, or
/// [`FIRST_LISTING_DAYS`] before it was connected.
fn listing_since(account: &Connector) -> DateTime<Utc> {
    account.cursor["since"]
        .as_str()
        .and_then(|since| DateTime::parse_from_rfc3339(since).ok())
        .map_or_else(
            || account.created_at - chrono::Duration::days(FIRST_LISTING_DAYS),
            |since| since.with_timezone(&Utc),
        )
}

/// The listing's keys of the cursor after a listing whose newest meeting started at `newest`,
/// which `connector::record_poll` merges over the stored cursor: a connector another loop also
/// reads, such as Teams' chats, keeps that loop's keys. It never moves backwards, and listings
/// overlap by that one meeting, which the account already knows.
fn next_cursor(cursor: &Value, newest: Option<DateTime<Utc>>) -> Value {
    let previous = cursor["since"]
        .as_str()
        .and_then(|since| DateTime::parse_from_rfc3339(since).ok())
        .map(|since| since.with_timezone(&Utc));
    match previous.max(newest) {
        Some(since) => json!({ "since": since.to_rfc3339() }),
        None => json!({}),
    }
}

/// A meeting `account` listed, and its first capture: the event as its source describes it, and
/// the record of it the account read.
fn new_meeting(
    account: &Connector,
    kind: ConnectorKind,
    listed: &ListedMeeting,
) -> (NewMeeting, NewMeetingCapture) {
    let meeting_id = Uuid::now_v7();
    let meeting = NewMeeting {
        id: meeting_id,
        title: listed.title.chars().take(2048).collect(),
        started_at: listed.started_at,
        duration_ms: listed.duration_ms,
        organizer: listed.organizer.chars().take(512).collect(),
        participants: json!(listed.participants),
        created_by: account.created_by,
    };
    let capture = NewMeetingCapture {
        id: Uuid::now_v7(),
        meeting_id,
        connector_id: Some(account.id),
        source_kind: kind.as_str().to_owned(),
        external_id: Some(listed.external_id.clone()),
        url: listed.url.clone().filter(|url| url.len() <= 4096),
        created_by: account.created_by,
    };
    (meeting, capture)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(cursor: Value) -> Connector {
        Connector {
            id: Uuid::now_v7(),
            kind: "fireflies".to_owned(),
            browser_session_id: None,
            sign_in_state: "unknown".to_owned(),
            cursor,
            last_polled_at: None,
            last_error: None,
            is_active: true,
            created_by: Uuid::now_v7(),
            created_at: "2026-10-21T12:00:00Z".parse().expect("a time"),
            updated_at: "2026-10-21T12:00:00Z".parse().expect("a time"),
            storage_location_id: Some(Uuid::now_v7()),
            token_account_id: Some(Uuid::now_v7()),
            jira_credential_id: None,
            github_credential_id: None,
            content: json!({}),
            syncing_since: None,
        }
    }

    #[test]
    fn a_first_listing_reaches_two_weeks_back_and_later_ones_start_at_the_cursor() {
        let fresh = account(json!({}));
        assert_eq!(
            listing_since(&fresh).to_rfc3339(),
            "2026-10-07T12:00:00+00:00"
        );
        let read = account(json!({ "since": "2026-10-20T09:30:00+00:00" }));
        assert_eq!(
            listing_since(&read).to_rfc3339(),
            "2026-10-20T09:30:00+00:00"
        );
    }

    #[test]
    fn the_cursor_never_moves_backwards() {
        let cursor = json!({ "since": "2026-10-20T09:30:00+00:00" });
        let older: DateTime<Utc> = "2026-10-19T09:30:00Z".parse().expect("a time");
        let newer: DateTime<Utc> = "2026-10-21T09:30:00Z".parse().expect("a time");

        assert_eq!(next_cursor(&cursor, Some(older)), cursor);
        assert_eq!(
            next_cursor(&cursor, Some(newer)),
            json!({ "since": "2026-10-21T09:30:00+00:00" })
        );
        assert_eq!(next_cursor(&json!({}), None), json!({}));
    }

    #[test]
    fn the_listing_writes_only_its_own_keys_of_the_cursor() {
        // A Teams connector's cursor also holds its chat reader's keys, which the listing must
        // neither drop nor put back as it read them.
        let shared = json!({
            "since": "2026-10-20T09:30:00+00:00",
            "baselined": true,
            "reader": { "since": 1_730_000_000_000_i64 },
        });
        let newer: DateTime<Utc> = "2026-10-21T09:30:00Z".parse().expect("a time");
        assert_eq!(
            next_cursor(&shared, Some(newer)),
            json!({ "since": "2026-10-21T09:30:00+00:00" })
        );
        assert_eq!(next_cursor(&json!({ "baselined": true }), None), json!({}));
    }
}
