// Copyright © 2026 Jalapeno Labs

//! Meetings: recordings and transcripts read from a meeting source as their person, cut into
//! topic segments that land on the timeline. See `docs/meetings.md`.
//!
//! # The loop
//!
//! [`Meetings::start`] runs until shutdown:
//!
//! - Every [`POLL_INTERVAL`] it lists each active meeting account's new meetings
//!   ([`source::MeetingSource::list`]) and stores each one it has not seen as a `pending`
//!   meeting. The account's cursor remembers the newest start time it saw.
//! - Every [`PROCESS_INTERVAL`] it carries each unfinished meeting as far as it goes:
//!   1. `pending`: fetch the transcript and a fresh recording address, download the
//!      recording into the workspace, and upload it to the account's storage location.
//!   2. `recorded`: Elysia reads the transcript in a text turn and cuts it into topic
//!      segments, each attributed to projects ([`segmentation`]).
//!   3. `segmented`: `ffmpeg` cuts each segment's clip in the workspace ([`media`]), the
//!      clips are uploaded, and each segment becomes a timeline record, with its transcript
//!      excerpt and summary, attributed to its projects, in one transaction.
//!
//! A step that fails is recorded on the meeting and tried again on the next pass, until it
//! has failed `models::meeting::MAX_ATTEMPTS` times; the person can retry it from there.
//! Every step is safe to repeat: storage paths are fixed per meeting, and records are written
//! only once every clip is up.
//!
//! # What is published
//!
//! `meeting.changed` carries a meeting's id, state, and owner, never its title or transcript:
//! every client of the event stream hears it. Each segment's record publishes
//! `timelineRecord.created`, which is where Elysia's intake hears of it.

pub mod fireflies;
pub mod media;
pub mod segmentation;
pub mod source;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, anyhow};
use chrono::{DateTime, Utc};
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use self::fireflies::Fireflies;
use self::segmentation::{MeetingContext, ProjectChoice, Segment};
use self::source::{ListedMeeting, MeetingSource, SourceError, TranscriptLine, is_public_download};
use crate::connectors::{ConnectorKind, SignInState};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::connector_account::{self, ConnectorAccount, PollOutcome};
use crate::models::meeting::{self, Meeting, MeetingState, NewMeeting, StoredRecording};
use crate::models::project;
use crate::models::storage_location::{self, StorageLocation};
use crate::models::timeline_record::{self, NewTimelineRecord, TimelineRecord};
use crate::realtime::{EventBus, ServerEvent};
use crate::routes::v1::connectors::ConnectorAccountResponse;
use crate::runner::workspace::WorkspaceContainer;
use crate::runner::{
    ConversationId, Runner, RunnerEvent, TurnMode, TurnRequest, TurnStatus, WorkspacePath,
};
use crate::storage::Storage;

/// How often each account's new meetings are listed. A meeting is useful on the timeline
/// within minutes of ending, not seconds, and Fireflies' smallest plan allows 500 calls a
/// day: ten minutes uses 144 of them.
pub const POLL_INTERVAL: Duration = Duration::from_mins(10);

/// How often unfinished meetings are carried forward.
pub const PROCESS_INTERVAL: Duration = Duration::from_mins(1);

/// How far back an account's first listing reaches, so a newly connected account brings its
/// recent meetings with it, but not its whole history.
const FIRST_LISTING_DAYS: i64 = 14;

/// Where segmentation turns run. They use no tools, so the directory only needs to exist.
const SEGMENTATION_DIRECTORY: &str = ".elysium/segmentation";

/// What a segment's timeline record is.
pub const SEGMENT_SOURCE_KIND: &str = "meeting";

/// Who attributes a segment to its projects, in the actor form history uses.
const ATTRIBUTED_BY: &str = "elysia";

/// The longest `actor` a record takes: the segment's speakers.
const ACTOR_MAX_CHARACTERS: usize = 512;

/// How many redirects a recording download may follow, each to an `https` named host.
const MAX_REDIRECTS: usize = 5;

#[derive(Clone)]
pub struct Meetings {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    events: EventBus,
    cipher: Arc<Cipher>,
    storage: Storage,
    runner: Runner,
    workspace: WorkspaceContainer,
    fireflies: Fireflies,
    /// For recording downloads: follows redirects only to `https` named hosts, so a
    /// source's address can never lead Elysium into its own network.
    downloads: reqwest::Client,
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
    pub cipher: Arc<Cipher>,
    pub storage: Storage,
    pub runner: Runner,
    pub workspace: WorkspaceContainer,
    pub http: reqwest::Client,
}

impl Meetings {
    /// # Errors
    /// Fails when the download client cannot be built.
    pub fn new(deps: MeetingsDeps) -> anyhow::Result<Self> {
        let downloads = reqwest::Client::builder()
            .user_agent(concat!("elysium-api/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= MAX_REDIRECTS {
                    attempt.error("too many redirects")
                } else if is_public_download(attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .build()
            .context("cannot build the recording download client")?;
        Ok(Self {
            inner: Arc::new(Inner {
                fireflies: Fireflies::new(deps.http),
                database: deps.database,
                events: deps.events,
                cipher: deps.cipher,
                storage: deps.storage,
                runner: deps.runner,
                workspace: deps.workspace,
                downloads,
            }),
        })
    }

    /// The Fireflies client, for checking a key before an account is saved.
    pub fn fireflies(&self) -> &Fireflies {
        &self.inner.fireflies
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

    /// Lists every active meeting account's new meetings.
    async fn poll(&self) {
        let kinds = ConnectorKind::MEETINGS.map(ConnectorKind::as_str);
        let accounts = match self.connection().await {
            Ok(mut connection) => connector_account::list_active(&mut connection, &kinds)
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
                    "cannot list the meeting accounts to read",
                );
                return;
            }
        };
        for account in accounts {
            if let Err(error) = self.poll_account(&account).await {
                event!(
                    name: "meetings.account.failed",
                    Level::WARN,
                    connector.account.id = %account.id,
                    error.message = %format!("{error:#}"),
                    "cannot record a meeting account's listing",
                );
            }
        }
    }

    /// Lists one account's new meetings, stores them, and records how the listing went.
    async fn poll_account(&self, account: &ConnectorAccount) -> anyhow::Result<()> {
        let credential = account
            .credential(&self.inner.cipher)
            .context("the account's credential cannot be decrypted")?
            .context("a meeting account signs in with a credential, and this one has none")?;
        let since = listing_since(account);

        let outcome = match self
            .inner
            .fireflies
            .list(&credential, Some(since), fireflies::MAX_LISTING)
            .await
        {
            Ok(listed) => {
                let newest = self.store_listed(account, &listed).await?;
                PollOutcome {
                    sign_in_state: SignInState::SignedIn.as_str().to_owned(),
                    cursor: next_cursor(&account.cursor, newest),
                    last_polled_at: Some(Utc::now()),
                    last_error: None,
                }
            }
            Err(error) => PollOutcome {
                sign_in_state: match error {
                    SourceError::Unauthorized(_) => SignInState::SignedOut,
                    SourceError::NotFound | SourceError::Refused(_) => SignInState::Unknown,
                }
                .as_str()
                .to_owned(),
                cursor: account.cursor.clone(),
                last_polled_at: Some(Utc::now()),
                last_error: Some(error.to_string()),
            },
        };

        let mut connection = self.connection().await?;
        let updated = connector_account::record_poll(&mut connection, account.id, outcome).await?;
        if updated.sign_in_state != account.sign_in_state
            || updated.last_error != account.last_error
        {
            self.inner
                .events
                .publish(&ServerEvent::ConnectorAccountUpserted(
                    ConnectorAccountResponse::new(updated),
                ));
        }
        Ok(())
    }

    /// Stores each listed meeting the account has not read, and answers the newest start time
    /// the listing held.
    async fn store_listed(
        &self,
        account: &ConnectorAccount,
        listed: &[ListedMeeting],
    ) -> anyhow::Result<Option<DateTime<Utc>>> {
        let mut connection = self.connection().await?;
        let external_ids: Vec<String> = listed
            .iter()
            .map(|meeting| meeting.external_id.clone())
            .collect();
        let known = meeting::known_external_ids(&mut connection, account.id, &external_ids).await?;
        let kind = ConnectorKind::parse(&account.kind)
            .with_context(|| format!("unknown connector kind {:?}", account.kind))?;

        let mut newest: Option<DateTime<Utc>> = None;
        for listed_meeting in listed {
            newest = newest.max(Some(listed_meeting.started_at));
            if known.contains(&listed_meeting.external_id) {
                continue;
            }
            let stored =
                meeting::insert_new(&mut connection, new_meeting(account, kind, listed_meeting))
                    .await?;
            if let Some(stored) = stored {
                self.publish(stored);
            }
        }
        Ok(newest)
    }

    /// Carries every unfinished meeting as far as it goes.
    async fn process(&self) {
        let unfinished = match self.connection().await {
            Ok(mut connection) => meeting::list_unfinished(&mut connection)
                .await
                .map_err(anyhow::Error::from),
            Err(error) => Err(error),
        };
        let unfinished = match unfinished {
            Ok(unfinished) => unfinished,
            Err(error) => {
                event!(
                    name: "meetings.process.failed",
                    Level::WARN,
                    error.message = %format!("{error:#}"),
                    "cannot list the meetings to process",
                );
                return;
            }
        };
        for unfinished_meeting in unfinished {
            self.carry(unfinished_meeting).await;
        }
    }

    /// Runs a meeting's steps one after another until it is done, or one fails.
    async fn carry(&self, mut current: Meeting) {
        loop {
            let id = current.id;
            let step = match current.state() {
                MeetingState::Pending => self.record(&current).await,
                MeetingState::Recorded => self.segment(&current).await,
                MeetingState::Segmented => self.publish_segments(&current).await,
                MeetingState::Done | MeetingState::Failed => return,
            };
            match step {
                Ok(advanced) => {
                    self.publish(advanced.clone());
                    current = advanced;
                }
                Err(error) => {
                    let message = format!("{error:#}");
                    event!(
                        name: "meetings.step.failed",
                        Level::WARN,
                        meeting.id = %id,
                        meeting.state = current.state().as_str(),
                        error.message = %message,
                        "a meeting's processing step failed; the next pass tries again",
                    );
                    match self.record_failure(&current, &message).await {
                        Ok(failed) => self.publish(failed),
                        Err(error) => event!(
                            name: "meetings.failure.unrecorded",
                            Level::ERROR,
                            meeting.id = %id,
                            error.message = %format!("{error:#}"),
                            "cannot record a meeting's failed step",
                        ),
                    }
                    return;
                }
            }
        }
    }

    async fn record_failure(&self, current: &Meeting, message: &str) -> anyhow::Result<Meeting> {
        let mut connection = self.connection().await?;
        Ok(meeting::record_failure(&mut connection, current, message).await?)
    }

    /// `pending` to `recorded`: the transcript, and the recording uploaded to storage.
    async fn record(&self, current: &Meeting) -> anyhow::Result<Meeting> {
        let account = self.account_of(current).await?;
        let credential = account
            .credential(&self.inner.cipher)
            .context("the account's credential cannot be decrypted")?
            .context("the meeting's account has no credential")?;
        let fetched = self
            .inner
            .fireflies
            .fetch(&credential, &current.external_id)
            .await?;
        let transcript = serde_json::to_value(&fetched.transcript)?;

        let stored = match (&fetched.recording, account.storage_location_id) {
            (Some(recording), Some(storage_location_id)) => {
                let (location, access_key) = self.location(storage_location_id).await?;
                let local = media::download(
                    &self.inner.downloads,
                    &self.inner.workspace,
                    current.id,
                    recording,
                )
                .await?;
                let path = media::recording_path(current.id, media::extension_of(&local.file_name));
                let kept = media::Kept {
                    storage: &self.inner.storage,
                    location: &location,
                    access_key: &access_key,
                };
                media::upload(
                    kept,
                    &self.inner.workspace,
                    current.id,
                    &local.file_name,
                    &path,
                    &local.content_type,
                )
                .await?;
                Some(StoredRecording {
                    storage_location_id,
                    path,
                    content_type: Some(local.content_type),
                })
            }
            _ => None,
        };

        let mut connection = self.connection().await?;
        Ok(meeting::record_recording(&mut connection, current.id, transcript, stored).await?)
    }

    /// `recorded` to `segmented`: Elysia's topic segments.
    async fn segment(&self, current: &Meeting) -> anyhow::Result<Meeting> {
        let transcript = transcript_of(current)?;
        let segments = if transcript.is_empty() {
            Vec::new()
        } else {
            let projects = {
                let mut connection = self.connection().await?;
                project::list(&mut connection).await?
            };
            let choices: Vec<ProjectChoice> = projects
                .into_iter()
                .map(|project| ProjectChoice {
                    id: project.id,
                    name: project.name,
                    description: project.description,
                })
                .collect();
            self.ask_for_segments(current, &transcript, &choices)
                .await?
        };

        let mut connection = self.connection().await?;
        Ok(meeting::record_segments(
            &mut connection,
            current.id,
            serde_json::to_value(&segments)?,
        )
        .await?)
    }

    /// One segmentation turn, and its answer checked.
    async fn ask_for_segments(
        &self,
        current: &Meeting,
        transcript: &[TranscriptLine],
        choices: &[ProjectChoice],
    ) -> anyhow::Result<Vec<Segment>> {
        let duration_ms = duration_of(current, transcript);
        let participants: Vec<String> =
            serde_json::from_value(current.participants.clone()).unwrap_or_default();
        let prompt = segmentation::prompt(
            MeetingContext {
                title: &current.title,
                duration_ms,
                participants: &participants,
            },
            transcript,
            choices,
        );
        let request = TurnRequest {
            conversation: ConversationId::new(Uuid::now_v7()),
            label: format!("meeting:{}", current.id),
            prompt,
            working_directory: WorkspacePath::new(SEGMENTATION_DIRECTORY)
                .expect("the segmentation directory is a plain relative path"),
            resume: None,
            mcp_servers: Vec::new(),
            allowed_tools: None,
            system_prompt_append: None,
            environment: Vec::new(),
            model: Some(segmentation::SEGMENTATION_MODEL.to_owned()),
            mode: TurnMode::Text {
                system_prompt: segmentation::SYSTEM_PROMPT.to_owned(),
            },
        };
        let mut ticket = self
            .inner
            .runner
            .submit(request)
            .map_err(|_shutting_down| anyhow!("Elysium is shutting down"))?;
        let outcome = loop {
            match ticket.events.recv().await {
                Some(RunnerEvent::Finished(outcome)) => break outcome,
                Some(_) => {}
                None => return Err(anyhow!("the segmentation turn ended without an outcome")),
            }
        };
        if outcome.status != TurnStatus::Completed {
            return Err(anyhow!(
                "the segmentation turn ended {:?}: {}",
                outcome.status,
                outcome.error.unwrap_or_default()
            ));
        }
        let answer = outcome.result.unwrap_or_default();
        Ok(segmentation::parse_answer(&answer, duration_ms, choices)?)
    }

    /// `segmented` to `done`: clips cut and uploaded, and each segment a timeline record.
    async fn publish_segments(&self, current: &Meeting) -> anyhow::Result<Meeting> {
        let transcript = transcript_of(current)?;
        let segments: Vec<Segment> =
            serde_json::from_value(current.segments.clone().unwrap_or_else(|| json!([])))
                .context("the meeting's segments are not in the stored shape")?;

        let clips = self.cut_clips(current, &segments).await?;
        let records: Vec<NewTimelineRecord> = segments
            .iter()
            .enumerate()
            .map(|(index, segment)| {
                segment_record(
                    current,
                    &transcript,
                    index,
                    segment,
                    clips.get(index).cloned().flatten(),
                )
            })
            .collect();

        let mut connection = self.connection().await?;
        let meeting_id = current.id;
        let (done, stored) = connection
            .transaction(async move |connection| {
                let mut stored: Vec<TimelineRecord> = Vec::new();
                for (record, segment) in records.into_iter().zip(&segments) {
                    let Some(record) = timeline_record::insert_new(connection, record).await?
                    else {
                        continue;
                    };
                    attribute(connection, &record, segment).await?;
                    stored.push(record);
                }
                let done = meeting::record_done(connection, meeting_id).await?;
                Ok::<_, diesel::result::Error>((done, stored))
            })
            .await?;
        drop(connection);

        for record in stored {
            self.inner
                .events
                .publish(&ServerEvent::TimelineRecordCreated {
                    id: record.id,
                    source_kind: record.source_kind,
                    created_by: record.created_by,
                    action_item_id: None,
                });
        }
        if let Err(error) = media::clean_up(&self.inner.workspace, current.id).await {
            event!(
                name: "meetings.cleanup.failed",
                Level::WARN,
                meeting.id = %current.id,
                error.message = %error,
                "a finished meeting's workspace directory could not be removed",
            );
        }
        Ok(done)
    }

    /// Cuts and uploads each segment's clip, answering where each landed, or nothing at all
    /// for a meeting without a recording.
    async fn cut_clips(
        &self,
        current: &Meeting,
        segments: &[Segment],
    ) -> anyhow::Result<Vec<Option<Clip>>> {
        let (Some(storage_location_id), Some(recording_path)) = (
            current.storage_location_id,
            current.recording_path.as_deref(),
        ) else {
            return Ok(Vec::new());
        };
        let content_type = current
            .recording_content_type
            .clone()
            .unwrap_or_else(|| "application/octet-stream".to_owned());
        let (location, access_key) = self.location(storage_location_id).await?;
        let kept = media::Kept {
            storage: &self.inner.storage,
            location: &location,
            access_key: &access_key,
        };
        let recording_name =
            media::ensure_local(kept, &self.inner.workspace, current.id, recording_path).await?;
        let extension = media::extension_of(recording_path);

        let mut clips = Vec::with_capacity(segments.len());
        for (index, segment) in segments.iter().enumerate() {
            let clip_name = format!("segment-{index}.{extension}");
            media::cut(
                &self.inner.workspace,
                current.id,
                &recording_name,
                segment.start_ms,
                segment.end_ms,
                &clip_name,
            )
            .await?;
            let path = media::clip_path(current.id, index, extension);
            media::upload(
                kept,
                &self.inner.workspace,
                current.id,
                &clip_name,
                &path,
                &content_type,
            )
            .await?;
            clips.push(Some(Clip {
                storage_location_id,
                path,
                content_type: content_type.clone(),
            }));
        }
        Ok(clips)
    }

    async fn account_of(&self, current: &Meeting) -> anyhow::Result<ConnectorAccount> {
        let account_id = current
            .connector_account_id
            .context("the meeting's account was removed")?;
        let mut connection = self.connection().await?;
        connector_account::find_owned(&mut connection, account_id, current.created_by)
            .await
            .context("the meeting's account was removed")
    }

    async fn location(
        &self,
        storage_location_id: Uuid,
    ) -> anyhow::Result<(StorageLocation, secrecy::SecretString)> {
        let mut connection = self.connection().await?;
        let location = storage_location::find(&mut connection, storage_location_id).await?;
        let access_key = location
            .access_key(&self.inner.cipher)
            .context("the storage location's access key cannot be decrypted")?;
        Ok((location, access_key))
    }

    fn publish(&self, changed: Meeting) {
        self.inner.events.publish(&ServerEvent::MeetingChanged {
            id: changed.id,
            state: changed.state,
            created_by: changed.created_by,
        });
    }
}

/// Where a segment's clip is kept.
#[derive(Debug, Clone)]
struct Clip {
    storage_location_id: Uuid,
    path: String,
    content_type: String,
}

/// Elysia's attributions for a segment's record.
async fn attribute(
    connection: &mut AsyncPgConnection,
    record: &TimelineRecord,
    segment: &Segment,
) -> diesel::QueryResult<()> {
    for project in &segment.projects {
        timeline_record::attribute(
            connection,
            record.id,
            project.project_id,
            project.confidence,
            ATTRIBUTED_BY,
        )
        .await?;
    }
    Ok(())
}

/// Where an account's next listing starts: its cursor's newest start time, or
/// [`FIRST_LISTING_DAYS`] before it was connected.
fn listing_since(account: &ConnectorAccount) -> DateTime<Utc> {
    account.cursor["since"]
        .as_str()
        .and_then(|since| DateTime::parse_from_rfc3339(since).ok())
        .map_or_else(
            || account.created_at - chrono::Duration::days(FIRST_LISTING_DAYS),
            |since| since.with_timezone(&Utc),
        )
}

/// The cursor after a listing whose newest meeting started at `newest`. It never moves
/// backwards, and listings overlap by that one meeting, which the account already knows.
fn next_cursor(cursor: &Value, newest: Option<DateTime<Utc>>) -> Value {
    let previous = cursor["since"]
        .as_str()
        .and_then(|since| DateTime::parse_from_rfc3339(since).ok())
        .map(|since| since.with_timezone(&Utc));
    match previous.max(newest) {
        Some(since) => json!({ "since": since.to_rfc3339() }),
        None => cursor.clone(),
    }
}

fn new_meeting(
    account: &ConnectorAccount,
    kind: ConnectorKind,
    listed: &ListedMeeting,
) -> NewMeeting {
    NewMeeting {
        id: Uuid::now_v7(),
        connector_account_id: Some(account.id),
        source_kind: kind.as_str().to_owned(),
        external_id: listed.external_id.clone(),
        title: listed.title.chars().take(2048).collect(),
        started_at: listed.started_at,
        duration_ms: listed.duration_ms,
        organizer: listed.organizer.chars().take(512).collect(),
        participants: json!(listed.participants),
        url: listed.url.clone().filter(|url| url.len() <= 4096),
        transcript: json!([]),
        created_by: account.created_by,
    }
}

fn transcript_of(current: &Meeting) -> anyhow::Result<Vec<TranscriptLine>> {
    serde_json::from_value(current.transcript.clone())
        .context("the meeting's transcript is not in the stored shape")
}

/// How long the meeting ran: what the source said, or where its transcript ends.
fn duration_of(current: &Meeting, transcript: &[TranscriptLine]) -> i64 {
    let spoken = transcript.iter().map(|line| line.end_ms).max().unwrap_or(0);
    current.duration_ms.unwrap_or(0).max(spoken)
}

/// A segment as a timeline record: its title, its transcript excerpt, its speakers, and the
/// span of the recording it covers, with its clip when there is one.
fn segment_record(
    current: &Meeting,
    transcript: &[TranscriptLine],
    index: usize,
    segment: &Segment,
    clip: Option<Clip>,
) -> NewTimelineRecord {
    let speakers = segmentation::speakers(transcript, segment).join(", ");
    let clip = clip.map(|clip| {
        json!({
            "storageLocationId": clip.storage_location_id,
            "path": clip.path,
            "contentType": clip.content_type,
        })
    });
    NewTimelineRecord {
        id: Uuid::now_v7(),
        source_kind: SEGMENT_SOURCE_KIND.to_owned(),
        connector_account_id: current.connector_account_id,
        external_id: Some(format!("{}#{index}", current.external_id)),
        occurred_at: current.started_at + chrono::Duration::milliseconds(segment.start_ms),
        actor: speakers.chars().take(ACTOR_MAX_CHARACTERS).collect(),
        title: segment.title.clone(),
        body: segmentation::excerpt(transcript, segment),
        url: current.url.clone(),
        media: Some(json!({
            "meetingId": current.id,
            "startMs": segment.start_ms,
            "endMs": segment.end_ms,
            "clip": clip,
        })),
        metadata: json!({
            "meetingId": current.id,
            "meetingTitle": current.title,
            "meetingSource": current.source_kind,
            "segmentIndex": index,
            "summary": segment.summary,
        }),
        created_by: current.created_by,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meetings::segmentation::SegmentProject;

    fn account(cursor: Value) -> ConnectorAccount {
        ConnectorAccount {
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
            credential_encrypted: Some(vec![1]),
            storage_location_id: Some(Uuid::now_v7()),
        }
    }

    fn meeting() -> Meeting {
        Meeting {
            id: Uuid::now_v7(),
            connector_account_id: Some(Uuid::now_v7()),
            source_kind: "fireflies".to_owned(),
            external_id: "01HX".to_owned(),
            title: "Weekly sync".to_owned(),
            started_at: "2026-10-21T15:00:00Z".parse().expect("a time"),
            duration_ms: Some(1_800_000),
            organizer: "ada@example.com".to_owned(),
            participants: json!(["ada@example.com"]),
            url: Some("https://app.fireflies.ai/view/01HX".to_owned()),
            transcript: json!([]),
            storage_location_id: None,
            recording_path: None,
            recording_content_type: None,
            segments: None,
            state: "segmented".to_owned(),
            attempts: 0,
            last_error: None,
            created_by: Uuid::now_v7(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
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
    fn a_segment_becomes_a_record_at_its_time_with_its_excerpt_and_clip() {
        let current = meeting();
        let transcript = vec![
            TranscriptLine {
                start_ms: 61_000,
                end_ms: 64_000,
                speaker: "Ada".to_owned(),
                text: "Launch is Friday.".to_owned(),
            },
            TranscriptLine {
                start_ms: 400_000,
                end_ms: 401_000,
                speaker: "Grace".to_owned(),
                text: "Later topic.".to_owned(),
            },
        ];
        let project_id = Uuid::now_v7();
        let segment = Segment {
            start_ms: 60_000,
            end_ms: 300_000,
            title: "Launch".to_owned(),
            summary: "Launch moves to Friday.".to_owned(),
            projects: vec![SegmentProject {
                project_id,
                confidence: 0.9,
            }],
        };
        let location = Uuid::now_v7();
        let record = segment_record(
            &current,
            &transcript,
            2,
            &segment,
            Some(Clip {
                storage_location_id: location,
                path: "meetings/x/segments/2.mp4".to_owned(),
                content_type: "video/mp4".to_owned(),
            }),
        );

        assert_eq!(record.source_kind, "meeting");
        assert_eq!(record.external_id.as_deref(), Some("01HX#2"));
        assert_eq!(record.occurred_at.to_rfc3339(), "2026-10-21T15:01:00+00:00");
        assert_eq!(record.actor, "Ada");
        assert_eq!(record.body, "[1:01] Ada: Launch is Friday.\n");
        assert_eq!(record.created_by, current.created_by);
        let media = record.media.expect("a span");
        assert_eq!(media["startMs"], 60_000);
        assert_eq!(media["clip"]["path"], "meetings/x/segments/2.mp4");
        assert_eq!(media["clip"]["storageLocationId"], json!(location));
        assert_eq!(record.metadata["summary"], "Launch moves to Friday.");
    }

    #[test]
    fn a_segment_without_a_recording_still_names_its_span() {
        let record = segment_record(
            &meeting(),
            &[],
            0,
            &Segment {
                start_ms: 0,
                end_ms: 60_000,
                title: "Intros".to_owned(),
                summary: String::new(),
                projects: Vec::new(),
            },
            None,
        );
        let media = record.media.expect("a span");
        assert_eq!(media["endMs"], 60_000);
        assert!(media["clip"].is_null());
    }

    #[test]
    fn a_meeting_runs_as_long_as_its_source_says_or_its_transcript_does() {
        let mut current = meeting();
        let line = TranscriptLine {
            start_ms: 0,
            end_ms: 2_000_000,
            speaker: String::new(),
            text: "x".to_owned(),
        };
        assert_eq!(
            duration_of(&current, std::slice::from_ref(&line)),
            2_000_000
        );
        current.duration_ms = None;
        assert_eq!(duration_of(&current, &[]), 0);
    }
}
