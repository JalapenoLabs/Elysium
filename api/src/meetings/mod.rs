// Copyright © 2026 Jalapeno Labs

//! Meetings: recordings and transcripts read from a meeting source as their person, cut into
//! topic segments that land on the timeline. See `docs/meetings.md`.
//!
//! # The loop
//!
//! [`Meetings::start`] runs until shutdown:
//!
//! - Every [`POLL_INTERVAL`] it lists each active meeting connector's new meetings
//!   ([`source::MeetingSource::list`]) and stores each one it has not seen as a `pending`
//!   meeting. The connector's cursor remembers the newest start time it saw.
//! - Every [`PROCESS_INTERVAL`] it carries each unfinished meeting as far as it goes:
//!   1. `pending`: start the meeting's run of the meeting workflow ([`workflow`]), whose
//!      [`fetch::FetchMeeting`] step stores the transcript and uploads the recording to the
//!      connector's storage location on the engine's step queue. A run that failed fails the
//!      meeting with its reason.
//!   2. `recorded`: Elysia reads the transcript in a text turn and cuts it into topic
//!      segments, each attributed to projects ([`segmentation`]).
//!   3. `segmented`: `ffmpeg` cuts each segment's clip in the workspace ([`media`]), the
//!      clips are uploaded, and each segment becomes a timeline record, with its transcript
//!      excerpt and summary, attributed to its projects, in one transaction.
//!
//! A meeting connector is one of any kind that provides meetings
//! (`kinds::providing(Provides::Meetings)`), and each is read through its kind's source
//! ([`sources::Sources::for_kind`]), so nothing here names a source.
//!
//! A segmenting or publishing step that fails is recorded on the meeting and tried again on the
//! next pass, until it has failed `models::meeting::MAX_ATTEMPTS` times; the person can retry it
//! from there. Every step is safe to repeat: storage paths are fixed per meeting, and records
//! are written only once every clip is up.
//!
//! # What is published
//!
//! `meeting.changed` carries a meeting's id, state, and owner, never its title or transcript:
//! every client of the event stream hears it. Each segment's record publishes
//! `timelineRecord.created`, which is where Elysia's intake hears of it.

pub mod fetch;
pub mod fireflies;
pub mod media;
pub mod segmentation;
pub mod source;
pub mod sources;
pub mod workflow;

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

use self::segmentation::{MeetingContext, ProjectChoice, Segment};
use self::source::{ListedMeeting, MeetingSource, SourceError, TranscriptLine};
use self::sources::{Source, Sources};
use crate::connectors::activity::Activity;
use crate::connectors::kinds::{self, Provides};
use crate::connectors::{ConnectorKind, SignInState};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::connector::{self, Connector, PollOutcome};
use crate::models::connector_run::ConnectorRun;
use crate::models::meeting::{self, Meeting, MeetingState, NewMeeting};
use crate::models::project;
use crate::models::storage_location::{self, StorageLocation};
use crate::models::timeline_record::{self, NewTimelineRecord, TimelineRecord};
use crate::models::token_account;
use crate::models::workflow_run::WorkflowRunState;
use crate::realtime::{EventBus, ServerEvent};
use crate::runner::workspace::WorkspaceContainer;
use crate::runner::{
    ConversationId, Runner, RunnerEvent, TurnMode, TurnRequest, TurnStatus, WorkspacePath,
};
use crate::storage::Storage;
use crate::workflows;

/// How often each connector's new meetings are listed. A meeting is useful on the timeline
/// within minutes of ending, not seconds, and the strictest source's smallest plan
/// (Fireflies', 500 calls a day) allows it: ten minutes uses 144 calls.
pub const POLL_INTERVAL: Duration = Duration::from_mins(10);

/// The most meetings one listing asks for. A source that allows fewer asks for what it allows,
/// and the cursor carries the rest to the next listing.
const LISTING_SIZE: u32 = 50;

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

#[derive(Clone)]
pub struct Meetings {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    events: EventBus,
    activity: Activity,
    cipher: Arc<Cipher>,
    storage: Storage,
    runner: Runner,
    workspace: WorkspaceContainer,
    sources: Sources,
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
    pub storage: Storage,
    pub runner: Runner,
    pub workspace: WorkspaceContainer,
    pub sources: Sources,
}

impl Meetings {
    pub fn new(deps: MeetingsDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                database: deps.database,
                events: deps.events,
                activity: deps.activity,
                cipher: deps.cipher,
                storage: deps.storage,
                runner: deps.runner,
                workspace: deps.workspace,
                sources: deps.sources,
            }),
        }
    }

    /// Lists one connector's new meetings now and carries every unfinished meeting forward,
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

    /// Lists one connector's new meetings, stores them, and records how the listing went.
    async fn poll_account(&self, account: &Connector) -> anyhow::Result<()> {
        let source = source_of(&self.inner.sources, account)?;
        let credential = {
            let mut connection = self.connection().await?;
            key_of(&mut connection, &self.inner.cipher, account).await?
        };
        let since = listing_since(account);
        let started = Utc::now();
        let mut run = None;

        let outcome = match source.list(&credential, Some(since), LISTING_SIZE).await {
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
                    cursor: account.cursor.clone(),
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
        let known = meeting::known_external_ids(&mut connection, account.id, &external_ids).await?;
        let kind = ConnectorKind::parse(&account.kind)
            .with_context(|| format!("unknown connector kind {:?}", account.kind))?;

        let mut newest: Option<DateTime<Utc>> = None;
        let mut stored_count = 0;
        for listed_meeting in listed {
            newest = newest.max(Some(listed_meeting.started_at));
            if known.contains(&listed_meeting.external_id) {
                continue;
            }
            let stored =
                meeting::insert_new(&mut connection, new_meeting(account, kind, listed_meeting))
                    .await?;
            if let Some(stored) = stored {
                stored_count += 1;
                self.publish(stored);
            }
        }
        Ok((newest, stored_count))
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
            if unfinished_meeting.state() != MeetingState::Pending {
                self.carry(unfinished_meeting).await;
                continue;
            }
            if let Err(error) = self.follow_fetch(&unfinished_meeting).await {
                event!(
                    name: "meetings.fetch.unfollowed",
                    Level::WARN,
                    meeting.id = %unfinished_meeting.id,
                    error.message = %format!("{error:#}"),
                    "cannot start or follow a meeting's run; the next pass tries again",
                );
            }
        }
    }

    /// Starts `pending`'s run of the meeting workflow, which fetches it, or follows the run it
    /// has: starting a run answers the live one when there is one. A run that failed fails the
    /// meeting with its reason, as a failed run of its connector, so the Meetings page and the
    /// connector say why; retrying the meeting retries the run (`workflow::retry`).
    async fn follow_fetch(&self, pending: &Meeting) -> anyhow::Result<()> {
        let mut connection = self.connection().await?;
        let started = workflows::start(
            &mut connection,
            &self.inner.events,
            workflow::run_for(pending.id),
        )
        .await?;
        let run = started.settled.run;
        if started.created {
            event!(
                name: "meetings.fetch.started",
                Level::INFO,
                meeting.id = %pending.id,
                workflow_run.id = %run.id,
                "a meeting's run started; it fetches the meeting",
            );
            return Ok(());
        }
        if run.state != WorkflowRunState::Failed {
            return Ok(());
        }

        let reason = run
            .error
            .unwrap_or_else(|| "Its run of the meeting workflow failed".to_owned());
        let failed = meeting::record_fetch_failure(&mut connection, pending.id, &reason).await?;
        drop(connection);
        let Some(failed) = failed else {
            event!(
                name: "meetings.fetch.moved_on",
                Level::DEBUG,
                meeting.id = %pending.id,
                "a meeting's run failed, but the meeting moved on meanwhile; it is left as it is",
            );
            return Ok(());
        };
        self.publish(failed.clone());
        if let Some(connector_id) = failed.connector_id {
            let started_at = run.started_at.unwrap_or(run.created_at);
            self.inner
                .activity
                .record(ConnectorRun::failed(connector_id, started_at, &reason))
                .await;
        }
        Ok(())
    }

    /// Runs a fetched meeting's steps one after another until it is done, or one fails. Its
    /// connector shows as syncing meanwhile, and a meeting that ends done or failed is a run of
    /// it.
    async fn carry(&self, mut current: Meeting) {
        let started = Utc::now();
        let _syncing = match current.connector_id {
            Some(connector_id) => self.inner.activity.begin(connector_id).await,
            None => None,
        };
        loop {
            let id = current.id;
            let step = match current.state() {
                MeetingState::Recorded => self.segment(&current).await,
                MeetingState::Segmented => self.publish_segments(&current).await,
                // A pending meeting is fetched by its run (`follow_fetch`).
                MeetingState::Pending | MeetingState::Done | MeetingState::Failed => return,
            };
            match step {
                Ok(advanced) => {
                    self.publish(advanced.clone());
                    if advanced.state() == MeetingState::Done
                        && let Some(connector_id) = advanced.connector_id
                    {
                        let segments = advanced
                            .segments
                            .as_ref()
                            .and_then(Value::as_array)
                            .map_or(0, Vec::len);
                        self.inner
                            .activity
                            .record(ConnectorRun::succeeded(connector_id, started, segments))
                            .await;
                    }
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
                    if let Some(connector_id) = current.connector_id {
                        self.inner
                            .activity
                            .record(ConnectorRun::failed(connector_id, started, &message))
                            .await;
                    }
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
            effort: None,
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
        let (location, access_key) = {
            let mut connection = self.connection().await?;
            location(&mut connection, &self.inner.cipher, storage_location_id).await?
        };
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

    fn publish(&self, changed: Meeting) {
        self.inner.events.publish(&ServerEvent::MeetingChanged {
            id: changed.id,
            state: changed.state,
            created_by: changed.created_by,
        });
    }
}

/// The source `connector`'s kind reads its meetings from.
fn source_of(sources: &Sources, connector: &Connector) -> anyhow::Result<Source> {
    ConnectorKind::parse(&connector.kind)
        .and_then(|kind| sources.for_kind(kind))
        .with_context(|| format!("connector kind {:?} provides no meetings", connector.kind))
}

/// The API key `connector` reads its meetings with, from the token account it reads through.
/// Every source today signs in with a key; a source read through a browser will resolve its
/// sign-in beside its source instead.
async fn key_of(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    connector: &Connector,
) -> anyhow::Result<secrecy::SecretString> {
    let token_account_id = connector
        .token_account_id
        .context("a meeting connector reads with an API key, and this one has none")?;
    let account = token_account::find(connection, token_account_id).await?;
    account
        .credential(cipher)
        .context("the connector's key cannot be decrypted")
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
fn listing_since(account: &Connector) -> DateTime<Utc> {
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

fn new_meeting(account: &Connector, kind: ConnectorKind, listed: &ListedMeeting) -> NewMeeting {
    NewMeeting {
        id: Uuid::now_v7(),
        connector_id: Some(account.id),
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
        connector_id: current.connector_id,
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

    fn meeting() -> Meeting {
        Meeting {
            id: Uuid::now_v7(),
            connector_id: Some(Uuid::now_v7()),
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
            transcript_hash: None,
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
