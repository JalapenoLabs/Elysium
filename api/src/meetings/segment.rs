// Copyright © 2026 Jalapeno Labs

//! The meeting workflow's last step, [`SegmentMeeting`]: what was said, cut into topics, each
//! attributed to the projects it is about, onto each project's timeline.
//!
//! It takes either of two things to segment ([`TranscriptToSegment`]):
//!
//! - **A meeting** past its correction, as the correction hands it on. Its corrected copy, or the
//!   transcript its correction would follow when it has none, is cut into topic segments in one
//!   of Elysia's text turns; `ffmpeg` cuts each segment's clip from its capture's recording in the
//!   workspace, at the words Whisper heard nearest its ends; and each segment becomes a timeline
//!   record, with its transcript excerpt, summary, span, and clip, attributed to its projects, in
//!   one transaction that also marks the meeting done. A meeting whose correction was skipped has
//!   its final transcript built here (`super::final_transcript`), so every meeting segmented has
//!   one. Segments are timed on the capture's clock, which the clips are cut on; their records
//!   are on the meeting's.
//! - **A conversation** that `crate::conversations` gathered: direct messages and mentions
//!   already on the timeline, named by their records. Its messages are cut into topics the same
//!   way, by their numbers rather than by time, and each message is attributed to its topic's
//!   projects. No record is made: a conversation's messages are already its records, and a
//!   topic of one is a run of them, so a topic record would put every message on a project's
//!   timeline twice. The same answer names the promises the person made in it, each recorded on
//!   its project for that project's Project Manager to follow up (`crate::models::promise`),
//!   when the message it was made in can be answered through its connector. The request
//!   workflow starts at this step with a conversation.
//! - **An action item** that arrived in no project, such as an issue assigned to its person. It
//!   has no topics: one turn reads which projects it belongs to, and it joins them with Elysia's
//!   confidence, led by the surest ([`item`]). The assigned issue workflow is this step alone.
//!
//! What it gives ([`Segmented`]) names every topic's span, projects, and records, for the steps
//! after it, which read what landed on which project.
//!
//! # Doing it once
//!
//! A meeting's segments are asked for once: the first answer is stored on the meeting
//! (`meeting::record_segments`) before any clip is cut, and an attempt that finds it cuts and
//! publishes from it without asking again. Clips go to fixed paths, records are unique per
//! connector and segment, and the meeting is marked done in the transaction that stores its
//! records, so a meeting already done answers what it published and changes nothing. A
//! conversation's segmentation is Elysia's whole reading of it: it replaces what she said of its
//! messages before, so reading it again, as it grows, never attributes a message twice, and a
//! message holds one promise at most, so it never records one twice either.
//!
//! # What it never overrides
//!
//! Every attribution goes through `models::attribution`, which records it in the record's
//! history and leaves alone whatever a person decided, so a segment a person moved stays moved.
//!
//! # What it logs
//!
//! Every company member can read a run's log, and meetings and messages are their owner's alone,
//! so the log carries counts, line numbers, and storage paths, never a title or a word said.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::anyhow;
use chrono::Utc;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::correct::{self, CorrectedMeeting};
use super::correction::{self, Base};
use super::final_transcript;
use super::media;
use super::segmentation::{self, ConversationSegment, MeetingContext, ProjectChoice, Segment};
use super::source::TranscriptLine;
use super::{SEGMENT_SOURCE_KIND, conversation};
use crate::agent_models::AgentKind;
use crate::connectors::ConnectorKind;
use crate::connectors::activity::Activity;
use crate::connectors::kinds::{self, Replies};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::attribution::{self, Attributor};
use crate::models::connector_run::ConnectorRun;
use crate::models::meeting::{self, Meeting, MeetingState};
use crate::models::meeting_capture::MeetingCapture;
use crate::models::meeting_correction::BasedOn;
use crate::models::meeting_transcription;
use crate::models::promise::{self, NewPromise, Promise};
use crate::models::timeline_record::{self, NewTimelineRecord, TimelineRecord};
use crate::models::{project, user};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::runner::events::{RunnerEvent, TurnStatus};
use crate::runner::workspace::WorkspaceContainer;
use crate::runner::{ConversationId, Runner, TurnMode, TurnRequest, WorkspacePath};
use crate::storage::Storage;
use crate::whisper::{WhisperTranscript, WhisperWord};
use crate::workflows::{Done, Step, StepContext, StepFailure, StepLog};

/// Where segmentation turns run. They use no tools, so the directory only needs to exist.
const SEGMENTATION_DIRECTORY: &str = ".elysium/segmentation";

/// The longest `actor` a record takes: the segment's speakers.
const ACTOR_MAX_CHARACTERS: usize = 512;

mod item;

/// What [`SegmentMeeting`] takes: a meeting past its correction, a conversation intake gathered,
/// or an action item to attribute.
///
/// Untagged, so each reads as its own JSON: a meeting is exactly what the correction gives
/// ([`CorrectedMeeting`]), and so is what the transcription gives when a person skips the
/// correction, while a conversation is the only shape with `recordIds`, and an item the only one
/// with `actionItemId`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TranscriptToSegment {
    Meeting(CorrectedMeeting),
    Conversation(ConversationToSegment),
    ActionItem(ItemToAttribute),
}

/// An action item to attribute to its projects, such as an issue that arrived assigned to its
/// person, in no project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemToAttribute {
    pub action_item_id: Uuid,
}

/// A conversation to segment: messages already on its person's timeline, which intake gathered
/// from one direct message thread or channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationToSegment {
    /// What the conversation is called where it was had, such as `#launch in Mooreslab`, which
    /// the prompt names it by.
    pub title: String,
    /// Its messages' timeline records, all one person's, in any order.
    pub record_ids: Vec<Uuid>,
}

/// What [`SegmentMeeting`] gives: every topic it found, in order. An action item reads as one
/// topic, titled as the item, holding the projects Elysia guessed for it, surest first.
///
/// A conversation not segmented, because a person skipped the step or no message of it was
/// left, reads as this with no topics and its `recordIds`, so the step after segmenting still
/// knows whose messages it was handed (`docs/engine.md`, A step).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segmented {
    #[serde(default)]
    pub segments: Vec<SegmentedTopic>,
    /// Every record segmenting read: a conversation's messages, those in no topic included.
    /// Empty for a meeting, whose records are its segments'.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub record_ids: Vec<Uuid>,
}

/// One topic, and where it landed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentedTopic {
    pub title: String,
    pub summary: String,
    /// Its span: from the recording's start for a meeting, from the first message for a
    /// conversation.
    pub start_ms: i64,
    pub end_ms: i64,
    /// What Elysia attributed it to, and how sure she was.
    pub projects: Vec<segmentation::SegmentProject>,
    /// Its records: a meeting segment's own, or a conversation topic's messages.
    pub record_ids: Vec<Uuid>,
}

/// Segments one meeting or conversation. See the module's docs.
#[derive(Clone)]
pub struct SegmentMeeting {
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
}

impl std::fmt::Debug for SegmentMeeting {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SegmentMeeting")
            .finish_non_exhaustive()
    }
}

/// What [`SegmentMeeting`] is built from.
#[derive(Clone)]
pub struct SegmentMeetingDeps {
    pub database: Pool,
    pub events: EventBus,
    pub activity: Activity,
    pub cipher: Arc<Cipher>,
    pub storage: Storage,
    pub runner: Runner,
    pub workspace: WorkspaceContainer,
}

/// Where a segment's clip is kept, and the stretch of the recording it holds.
#[derive(Debug, Clone)]
struct Clip {
    storage_location_id: Uuid,
    path: String,
    content_type: String,
    start_ms: i64,
    end_ms: i64,
}

impl SegmentMeeting {
    pub fn new(deps: SegmentMeetingDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                database: deps.database,
                events: deps.events,
                activity: deps.activity,
                cipher: deps.cipher,
                storage: deps.storage,
                runner: deps.runner,
                workspace: deps.workspace,
            }),
        }
    }

    async fn connection(
        &self,
    ) -> Result<diesel_async::pooled_connection::deadpool::Object<AsyncPgConnection>, StepFailure>
    {
        self.inner.database.get().await.map_err(|error| {
            StepFailure::retryable(format!("No database connection available: {error}"))
        })
    }

    /// Segments a meeting past its correction and publishes its segments. See the module's
    /// docs.
    async fn segment_meeting(
        &self,
        input: CorrectedMeeting,
        context: &StepContext,
    ) -> Result<Done<Segmented>, StepFailure> {
        let log = &context.log;
        let gone = |error| match error {
            diesel::result::Error::NotFound => {
                StepFailure::permanent("The meeting no longer exists")
            }
            other => StepFailure::retryable(other),
        };
        let (current, capture) = {
            let mut connection = self.connection().await?;
            let current = meeting::find(&mut connection, input.meeting_id)
                .await
                .map_err(gone)?;
            let capture = super::capture_of(&mut connection, input.meeting_id, input.capture_id)
                .await
                .map_err(gone)?;
            (current, capture)
        };
        match current.state() {
            MeetingState::Pending => {
                log.line("The meeting's fetch was skipped");
                return Ok(Done::Skipped {
                    reason: "Not fetched, so nothing to segment".to_owned(),
                });
            }
            MeetingState::Done => return self.already_published(&current, log).await,
            MeetingState::Recorded | MeetingState::Segmented => {}
        }

        let started = Utc::now();
        // The connector shows as syncing while one of its meetings is processed.
        let _syncing = match capture.connector_id {
            Some(connector_id) => self.inner.activity.begin(connector_id).await,
            None => None,
        };
        let whisper = self.whisper_transcript(&capture).await?;
        let heard = self.heard(&input, &capture, whisper.as_ref()).await?;
        self.ensure_final_transcript(&capture, &heard, log).await?;
        let transcript = heard.into_lines(log);
        let segments = match stored_segments(&current)? {
            Some(segments) => {
                log.line(&format!(
                    "Already segmented into {} topics; nothing is asked again",
                    segments.len()
                ));
                segments
            }
            None if transcript.is_empty() => return self.nothing_said(&current, log).await,
            None => {
                let asked = self.ask_meeting(&current, &transcript, context).await?;
                self.store_segments(current.id, asked, log).await?
            }
        };

        let words: Vec<WhisperWord> = whisper
            .into_iter()
            .flat_map(|heard| heard.segments)
            .flat_map(|segment| segment.words)
            .collect();
        let clips = self.cut_clips(&capture, &segments, &words, log).await?;
        let published = self
            .publish(&current, &capture, &transcript, &segments, clips)
            .await?;
        let Some(published) = published else {
            return self.published_by_another(current.id, log).await;
        };
        log.line(&format!(
            "Published {} topics to the timeline",
            published.created.len()
        ));
        self.announce_published(&current, &published.created);
        if let Some(connector_id) = capture.connector_id {
            self.inner
                .activity
                .record(ConnectorRun::succeeded(
                    connector_id,
                    started,
                    segments.len(),
                ))
                .await;
        }
        // Only cutting clips fills the meeting's workspace directory.
        let cut_any = capture.recording_path.is_some();
        if cut_any && let Err(error) = media::clean_up(&self.inner.workspace, current.id).await {
            // The clips are up and the meeting is done; the scratch copy left behind costs
            // workspace space and nothing else, so the step still succeeds.
            log.line(&format!(
                "The meeting's workspace directory could not be removed: {error}"
            ));
        }

        let attributed = segments
            .iter()
            .filter(|segment| !segment.projects.is_empty())
            .count();
        Ok(Done::Succeeded {
            output: meeting_topics(&segments, &published.record_ids),
            summary: Some(format!(
                "Put {} topics on the timeline, {attributed} of them on projects",
                segments.len()
            )),
        })
    }

    /// What `capture` heard, with Whisper's transcript `whisper`, and the corrected copy `input`
    /// names, when it names one that is stored.
    async fn heard(
        &self,
        input: &CorrectedMeeting,
        capture: &MeetingCapture,
        whisper: Option<&WhisperTranscript>,
    ) -> Result<Heard, StepFailure> {
        let mut connection = self.connection().await?;
        let corrected = correct::corrected_lines(&mut connection, input)
            .await
            .map_err(StepFailure::retryable)?;
        let source: Vec<TranscriptLine> = serde_json::from_value(capture.transcript.clone())
            .map_err(|error| {
                StepFailure::permanent(format!(
                    "The meeting's transcript is not in the stored shape: {error}"
                ))
            })?;
        Ok(Heard {
            base: correction::base(source, whisper),
            corrected,
        })
    }

    /// Builds the meeting's final transcript from what `capture` heard and its corrected copy,
    /// unless it has one: a meeting whose correction was skipped has none yet.
    async fn ensure_final_transcript(
        &self,
        capture: &MeetingCapture,
        heard: &Heard,
        log: &StepLog,
    ) -> Result<(), StepFailure> {
        let Some(base) = &heard.base else {
            return Ok(());
        };
        let mut connection = self.connection().await?;
        let built = final_transcript::ensure_built(
            &mut connection,
            capture,
            base,
            heard.corrected.as_deref(),
        )
        .await
        .map_err(StepFailure::retryable)?;
        if let Some(built) = built {
            log.line(&format!(
                "Built the final transcript: {} words",
                built.data["words"]
            ));
        }
        Ok(())
    }

    /// One segmentation turn over `transcript`, its answer checked.
    async fn ask_meeting(
        &self,
        current: &Meeting,
        transcript: &[TranscriptLine],
        context: &StepContext,
    ) -> Result<Vec<Segment>, StepFailure> {
        let choices = self.project_choices().await?;
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
            &choices,
        );
        let answer = self
            .ask(
                format!("meeting:{}:segmentation", current.id),
                prompt,
                segmentation::SYSTEM_PROMPT,
                context,
            )
            .await?;
        let segments =
            segmentation::parse_answer(&answer, duration_ms, &choices).map_err(|error| {
                StepFailure::retryable(format!("The segmentation's answer: {error}"))
            })?;
        let attributed = segments
            .iter()
            .filter(|segment| !segment.projects.is_empty())
            .count();
        context.log.line(&format!(
            "Found {} topics, {attributed} of them about a project",
            segments.len()
        ));
        Ok(segments)
    }

    /// Stores `asked` as the meeting's segments, or answers the segments another attempt stored
    /// first, which every later attempt cuts and publishes from.
    async fn store_segments(
        &self,
        meeting_id: Uuid,
        asked: Vec<Segment>,
        log: &StepLog,
    ) -> Result<Vec<Segment>, StepFailure> {
        let mut connection = self.connection().await?;
        let answer = serde_json::to_value(&asked).map_err(StepFailure::permanent)?;
        let written = meeting::record_segments(&mut connection, meeting_id, answer)
            .await
            .map_err(StepFailure::retryable)?;
        if written.is_some() {
            return Ok(asked);
        }
        let stored = meeting::find(&mut connection, meeting_id)
            .await
            .map_err(StepFailure::retryable)?;
        log.line("Another attempt stored its segments first; they are kept");
        stored_segments(&stored)?
            .ok_or_else(|| StepFailure::retryable("The meeting's segments went missing"))
    }

    /// How a meeting in which nothing was said ends: done, with no topics.
    async fn nothing_said(
        &self,
        current: &Meeting,
        log: &StepLog,
    ) -> Result<Done<Segmented>, StepFailure> {
        let mut connection = self.connection().await?;
        let meeting_id = current.id;
        let done = connection
            .transaction(async move |connection| {
                meeting::record_segments(connection, meeting_id, json!([])).await?;
                meeting::record_done(connection, meeting_id).await
            })
            .await
            .map_err(StepFailure::retryable)?;
        log.line("Neither transcript holds anything said");
        self.announce_meeting(&done);
        Ok(Done::Skipped {
            reason: "Nothing was said, so nothing to segment".to_owned(),
        })
    }

    /// How an attempt ends that found meeting `meeting_id` published by another attempt while it
    /// worked: with what that attempt published.
    async fn published_by_another(
        &self,
        meeting_id: Uuid,
        log: &StepLog,
    ) -> Result<Done<Segmented>, StepFailure> {
        log.line("Another attempt published the meeting first; nothing was stored twice");
        let done = {
            let mut connection = self.connection().await?;
            meeting::find(&mut connection, meeting_id)
                .await
                .map_err(StepFailure::retryable)?
        };
        self.already_published(&done, log).await
    }

    /// What a meeting published already gives, changing nothing.
    async fn already_published(
        &self,
        current: &Meeting,
        log: &StepLog,
    ) -> Result<Done<Segmented>, StepFailure> {
        let segments = stored_segments(current)?.unwrap_or_default();
        let records = {
            let mut connection = self.connection().await?;
            timeline_record::of_meeting(&mut connection, current.created_by, current.id)
                .await
                .map_err(StepFailure::retryable)?
        };
        let mut record_ids: Vec<Option<Uuid>> = vec![None; segments.len()];
        for (record, _attributions) in records {
            let index = record.metadata["segmentIndex"]
                .as_u64()
                .and_then(|index| usize::try_from(index).ok());
            if let Some(slot) = index.and_then(|index| record_ids.get_mut(index)) {
                *slot = Some(record.id);
            }
        }
        log.line(&format!(
            "Already on the timeline as {} topics; nothing was changed",
            segments.len()
        ));
        Ok(Done::Succeeded {
            output: meeting_topics(&segments, &record_ids),
            summary: Some(format!(
                "Already on the timeline as {} topics",
                segments.len()
            )),
        })
    }

    /// What Whisper heard in `capture`'s recording; `None` when it was not transcribed.
    async fn whisper_transcript(
        &self,
        capture: &MeetingCapture,
    ) -> Result<Option<WhisperTranscript>, StepFailure> {
        let stored = {
            let mut connection = self.connection().await?;
            meeting_transcription::find(&mut connection, capture.id)
                .await
                .map_err(StepFailure::retryable)?
        };
        stored
            .map(|stored| serde_json::from_value(stored.transcript))
            .transpose()
            .map_err(|error| {
                StepFailure::permanent(format!(
                    "The meeting's Whisper transcript is not in the stored shape: {error}"
                ))
            })
    }

    /// Cuts and uploads each segment's clip from `capture`'s recording, at Whisper's words when
    /// there are any, answering where each landed, or nothing at all for a capture without a
    /// recording. Segments and words are on the capture's own clock, as its recording is.
    async fn cut_clips(
        &self,
        capture: &MeetingCapture,
        segments: &[Segment],
        words: &[WhisperWord],
        log: &StepLog,
    ) -> Result<Vec<Option<Clip>>, StepFailure> {
        let meeting_id = capture.meeting_id;
        let (Some(storage_location_id), Some(recording_path)) = (
            capture.storage_location_id,
            capture.recording_path.as_deref(),
        ) else {
            log.line("No recording is kept, so the topics carry no clips");
            return Ok(Vec::new());
        };
        let content_type = capture
            .recording_content_type
            .clone()
            .unwrap_or_else(|| "application/octet-stream".to_owned());
        let (location, access_key) = {
            let mut connection = self.connection().await?;
            super::location(&mut connection, &self.inner.cipher, storage_location_id)
                .await
                .map_err(StepFailure::retryable)?
        };
        let kept = media::Kept {
            storage: &self.inner.storage,
            location: &location,
            access_key: &access_key,
        };
        let recording_name = media::ensure_local(
            kept,
            &self.inner.workspace,
            (meeting_id, capture.id),
            recording_path,
        )
        .await
        .map_err(StepFailure::retryable)?;
        let extension = media::extension_of(recording_path);

        let mut clips = Vec::with_capacity(segments.len());
        for (index, segment) in segments.iter().enumerate() {
            let (start_ms, end_ms) =
                segmentation::clip_span(segment.start_ms, segment.end_ms, words);
            let clip_name = format!("segment-{index}.{extension}");
            media::cut(
                &self.inner.workspace,
                meeting_id,
                &recording_name,
                start_ms,
                end_ms,
                &clip_name,
            )
            .await
            .map_err(StepFailure::retryable)?;
            let path = media::clip_path(meeting_id, index, extension);
            media::upload(
                kept,
                &self.inner.workspace,
                &media::directory(meeting_id),
                &clip_name,
                &path,
                &content_type,
            )
            .await
            .map_err(StepFailure::retryable)?;
            log.line(&format!("Uploaded a clip to {path}"));
            clips.push(Some(Clip {
                storage_location_id,
                path,
                content_type: content_type.clone(),
                start_ms,
                end_ms,
            }));
        }
        if words.is_empty() {
            log.line(&format!(
                "Cut {} clips at the transcript's times",
                clips.len()
            ));
        } else {
            log.line(&format!(
                "Cut {} clips at Whisper's word timings",
                clips.len()
            ));
        }
        Ok(clips)
    }

    /// Stores every segment as a timeline record, attributed to its projects, and marks the
    /// meeting done, in one transaction under the meeting's lock. Answers `None`, storing nothing,
    /// when another attempt marked the meeting done first: two attempts at once publish it once,
    /// whether or not the unique key on its records can tell them apart (a capture with no
    /// connector cannot).
    async fn publish(
        &self,
        current: &Meeting,
        capture: &MeetingCapture,
        transcript: &[TranscriptLine],
        segments: &[Segment],
        clips: Vec<Option<Clip>>,
    ) -> Result<Option<Published>, StepFailure> {
        let records: Vec<NewTimelineRecord> = segments
            .iter()
            .enumerate()
            .map(|(index, segment)| {
                let clip = clips.get(index).cloned().flatten();
                segment_record(current, capture, transcript, index, segment, clip)
            })
            .collect();
        let mut connection = self.connection().await?;
        let connector_id = capture.connector_id;
        let meeting_id = current.id;
        let published = connection
            .transaction(async move |connection| {
                if meeting::lock(connection, meeting_id).await?.state() == MeetingState::Done {
                    return Ok(None);
                }
                let existing = existing_projects(connection).await?;
                let mut published = Published::default();
                for (record, segment) in records.into_iter().zip(segments) {
                    let external_id = record.external_id.clone();
                    let Some(stored) = timeline_record::insert_new(connection, record).await?
                    else {
                        // Recorded already, as a meeting published before segmenting joined its
                        // run may have been; it keeps the attributions it has.
                        let found = match (connector_id, external_id) {
                            (Some(connector_id), Some(external_id)) => {
                                timeline_record::find_by_external_id(
                                    connection,
                                    connector_id,
                                    &external_id,
                                )
                                .await?
                            }
                            _ => None,
                        };
                        published.record_ids.push(found.map(|record| record.id));
                        continue;
                    };
                    for project in &segment.projects {
                        if !existing.contains(&project.project_id) {
                            continue;
                        }
                        attribution::attribute(
                            connection,
                            stored.id,
                            project.project_id,
                            project.confidence,
                            Attributor::Elysia,
                        )
                        .await?;
                    }
                    published.record_ids.push(Some(stored.id));
                    published.created.push(stored);
                }
                meeting::record_done(connection, meeting_id).await?;
                Ok::<_, diesel::result::Error>(Some(published))
            })
            .await
            .map_err(StepFailure::retryable)?;
        Ok(published)
    }

    /// Tells every client of each new segment's record and of the meeting that finished.
    fn announce_published(&self, current: &Meeting, created: &[TimelineRecord]) {
        for record in created {
            self.inner
                .events
                .publish(&ServerEvent::TimelineRecordCreated {
                    id: record.id,
                    source_kind: record.source_kind.clone(),
                    created_by: record.created_by,
                    action_item_id: None,
                    conversation_id: None,
                });
        }
        self.inner.events.publish(&ServerEvent::MeetingChanged {
            id: current.id,
            state: MeetingState::Done.as_str().to_owned(),
            created_by: current.created_by,
            entry_id: None,
        });
    }

    fn announce_meeting(&self, changed: &Meeting) {
        self.inner.events.publish(&ServerEvent::MeetingChanged {
            id: changed.id,
            state: changed.state.clone(),
            created_by: changed.created_by,
            entry_id: None,
        });
    }

    /// Segments a conversation's messages and attributes each to its topic's projects. See the
    /// module's docs.
    async fn segment_conversation(
        &self,
        input: ConversationToSegment,
        context: &StepContext,
    ) -> Result<Done<Segmented>, StepFailure> {
        let log = &context.log;
        let mut records = {
            let mut connection = self.connection().await?;
            timeline_record::find_many(&mut connection, &input.record_ids)
                .await
                .map_err(StepFailure::retryable)?
        };
        let missing = input.record_ids.len().saturating_sub(records.len());
        if missing > 0 {
            log.line(&format!(
                "{missing} of the conversation's messages are gone"
            ));
        }
        let Some(first) = records.first() else {
            return Ok(Done::Skipped {
                reason: "No message of the conversation is left to segment".to_owned(),
            });
        };
        let owner = first.created_by;
        if records.iter().any(|record| record.created_by != owner) {
            return Err(StepFailure::permanent(
                "The conversation's messages are on more than one person's timeline",
            ));
        }
        records.sort_by_key(|record| (record.occurred_at, record.id));
        let messages = conversation::lines_of(&records);
        log.line(&format!("Reading {} messages", messages.len()));

        let choices = self.project_choices().await?;
        let person = {
            let mut connection = self.connection().await?;
            user::find(&mut connection, owner)
                .await
                .map_err(StepFailure::retryable)?
                .name
        };
        let prompt = segmentation::conversation_prompt(&input.title, &person, &messages, &choices);
        let answer = self
            .ask(
                "conversation:segmentation".to_owned(),
                prompt,
                segmentation::CONVERSATION_SYSTEM_PROMPT,
                context,
            )
            .await?;
        let reading = segmentation::parse_conversation_answer(&answer, &messages, &choices)
            .map_err(|error| {
                StepFailure::retryable(format!("The segmentation's answer: {error}"))
            })?;
        let segments = reading.segments;
        let found = promises_to_keep(&reading.promises, &records);
        let unanswerable = reading.promises.len() - found.len();
        if unanswerable > 0 {
            log.line(&format!(
                "{unanswerable} promises were made where no follow-up can answer them"
            ));
        }

        let (changed, promised) = {
            let mut connection = self.connection().await?;
            let attributed = records.clone();
            let topics = segments.clone();
            connection
                .transaction(async move |connection| {
                    let existing = existing_projects(connection).await?;
                    let changed =
                        conversation::attribute(connection, &attributed, &topics, &existing)
                            .await?;
                    // One whose project was removed while the turn ran is left out.
                    let found: Vec<NewPromise> = found
                        .into_iter()
                        .filter(|promise| existing.contains(&promise.project_id))
                        .collect();
                    let promised = promise::record(connection, &found).await?;
                    diesel::QueryResult::Ok((changed, promised))
                })
                .await
                .map_err(StepFailure::retryable)?
        };
        log.line(&format!(
            "Found {} topics; the attributions of {} messages changed; {} new promises",
            segments.len(),
            changed.len(),
            promised.len()
        ));
        self.announce_reading(owner, &changed, &promised);
        Ok(Done::Succeeded {
            output: conversation_topics(&segments, &records, &messages),
            summary: Some(format!(
                "Found {} topics in {} messages",
                segments.len(),
                messages.len()
            )),
        })
    }

    /// Announces what reading a conversation of `owner`'s changed: the messages whose
    /// attributions moved, and the Project Managers of the projects it found promises on, which
    /// those promises woke.
    fn announce_reading(&self, owner: Uuid, changed: &[Uuid], promised: &[Promise]) {
        for &record_id in changed {
            self.inner
                .events
                .publish(&ServerEvent::TimelineRecordChanged {
                    id: record_id,
                    created_by: owner,
                });
        }
        let mut projects_woken: Vec<Uuid> =
            promised.iter().map(|promise| promise.project_id).collect();
        projects_woken.sort_unstable();
        projects_woken.dedup();
        for project_id in projects_woken {
            self.inner
                .events
                .publish(&ServerEvent::ProjectManagerChanged { project_id });
        }
        if !promised.is_empty() {
            self.inner.events.signal(&Signal::ProjectManagers);
        }
    }

    /// Every project, as a segment may be attributed to it.
    async fn project_choices(&self) -> Result<Vec<ProjectChoice>, StepFailure> {
        let mut connection = self.connection().await?;
        let projects = project::list(&mut connection)
            .await
            .map_err(StepFailure::retryable)?;
        Ok(projects
            .into_iter()
            .map(|project| ProjectChoice {
                id: project.id,
                name: project.name,
                description: project.description,
            })
            .collect())
    }

    /// One text turn on the step's model, answering the model's final text.
    async fn ask(
        &self,
        label: String,
        prompt: String,
        system_prompt: &str,
        context: &StepContext,
    ) -> Result<String, StepFailure> {
        // Read as each turn starts, so an admin's change applies from the next one.
        let model = context.model().await?;
        let request = TurnRequest {
            conversation: ConversationId::new(Uuid::now_v7()),
            label,
            prompt,
            working_directory: WorkspacePath::new(SEGMENTATION_DIRECTORY)
                .expect("the segmentation directory is a plain relative path"),
            resume: None,
            mcp_servers: Vec::new(),
            allowed_tools: None,
            system_prompt_append: None,
            environment: Vec::new(),
            model: Some(model.model),
            effort: Some(model.effort),
            mode: TurnMode::Text {
                system_prompt: system_prompt.to_owned(),
            },
        };
        let mut ticket = self
            .inner
            .runner
            .submit(request)
            .map_err(|_shutting_down| StepFailure::retryable("Elysium is shutting down"))?;
        let outcome = loop {
            match ticket.events.recv().await {
                Some(RunnerEvent::Finished(outcome)) => break outcome,
                Some(_) => {}
                None => {
                    return Err(StepFailure::retryable(
                        "The segmentation turn ended without an outcome",
                    ));
                }
            }
        };
        if outcome.prompt_too_long() {
            // The prompt is the transcript, conversation, or item itself, so every attempt would
            // fail the same way.
            return Err(StepFailure::permanent(format!(
                "Too long for the model to read in one turn: {}",
                outcome.error.unwrap_or_default()
            )));
        }
        if outcome.status != TurnStatus::Completed {
            return Err(StepFailure::retryable(anyhow!(
                "The segmentation turn ended {:?}: {}",
                outcome.status,
                outcome.error.unwrap_or_default()
            )));
        }
        Ok(outcome.result.unwrap_or_default())
    }
}

impl Step for SegmentMeeting {
    /// Named for the meetings it first segmented, which its model setting is filed under; it
    /// segments conversations as well.
    const NAME: &'static str = "segment_meeting";
    /// It reads the transcript and cuts it into topics in an agent turn.
    const AGENT_KIND: Option<AgentKind> = Some(AgentKind::Segmenting);
    type Input = TranscriptToSegment;
    type Output = Segmented;

    async fn run(
        &self,
        input: TranscriptToSegment,
        context: StepContext,
    ) -> Result<Done<Segmented>, StepFailure> {
        match input {
            TranscriptToSegment::Meeting(meeting) => self.segment_meeting(meeting, &context).await,
            TranscriptToSegment::Conversation(conversation) => {
                self.segment_conversation(conversation, &context).await
            }
            TranscriptToSegment::ActionItem(item) => self.attribute_item(item, &context).await,
        }
    }
}

#[cfg(test)]
impl SegmentMeeting {
    /// The step over `database`, with a runner and a workspace Docker never answers: enough for
    /// a meeting segmented already, one not fetched, or one in which nothing was said.
    pub fn for_test(database: Pool) -> Self {
        let docker = bollard::Docker::connect_with_http(
            "http://127.0.0.1:9",
            5,
            bollard::API_DEFAULT_VERSION,
        )
        .expect("a static address parses");
        let workspace = WorkspaceContainer::new(docker, "elysium-workspace:test".to_owned());
        let events = EventBus::in_process();
        let runner = Runner::new(
            workspace.clone(),
            database.clone(),
            Arc::new(crate::test_support::cipher()),
            crate::activity::Activities::new(),
            crate::claude_accounts::TurnSignal::default(),
            &tokio_util::sync::CancellationToken::new(),
        );
        Self::new(SegmentMeetingDeps {
            activity: Activity::new(database.clone(), events.clone()),
            database,
            events,
            cipher: Arc::new(crate::test_support::cipher()),
            storage: Storage::new(reqwest::Client::new()),
            runner,
            workspace,
        })
    }
}

/// What publishing a meeting's segments stored.
#[derive(Debug, Default)]
struct Published {
    /// The records this attempt created.
    created: Vec<TimelineRecord>,
    /// Each segment's record, in order; `None` where one recorded before cannot be found.
    record_ids: Vec<Option<Uuid>>,
}

/// What a meeting's capture heard, and its corrected copy when the run named one.
struct Heard {
    /// The transcript a correction follows: the source's lines, or Whisper's segments when the
    /// source sent none. `None` when nothing was heard.
    base: Option<Base>,
    corrected: Option<Vec<TranscriptLine>>,
}

impl Heard {
    /// The lines segmenting reads: the corrected copy, or, when the run handed on none, the
    /// transcript its correction would have followed.
    fn into_lines(self, log: &StepLog) -> Vec<TranscriptLine> {
        let (lines, which) = match (self.corrected, self.base) {
            (Some(lines), _) => (lines, "the corrected transcript"),
            (None, Some(base)) => {
                let which = match base.based_on {
                    BasedOn::Source => "the source's transcript",
                    BasedOn::Whisper => "Whisper's transcript",
                };
                (base.lines, which)
            }
            (None, None) => (Vec::new(), "either transcript"),
        };
        log.line(&format!("Reading {} lines of {which}", lines.len()));
        lines
    }
}

/// The segments stored on `current`, when it has them.
fn stored_segments(current: &Meeting) -> Result<Option<Vec<Segment>>, StepFailure> {
    let Some(stored) = &current.segments else {
        return Ok(None);
    };
    serde_json::from_value(stored.clone())
        .map(Some)
        .map_err(|error| {
            StepFailure::permanent(format!(
                "The meeting's segments are not in the stored shape: {error}"
            ))
        })
}

/// Every project's id now, so an attribution to one removed since the turn is left out rather
/// than failing the whole write.
async fn existing_projects(
    connection: &mut AsyncPgConnection,
) -> diesel::QueryResult<HashSet<Uuid>> {
    let projects = project::list(connection).await?;
    Ok(projects.into_iter().map(|project| project.id).collect())
}

/// A meeting's topics as the step gives them, each with its record.
fn meeting_topics(segments: &[Segment], record_ids: &[Option<Uuid>]) -> Segmented {
    let segments = segments
        .iter()
        .enumerate()
        .map(|(index, segment)| SegmentedTopic {
            title: segment.title.clone(),
            summary: segment.summary.clone(),
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            projects: segment.projects.clone(),
            record_ids: record_ids
                .get(index)
                .copied()
                .flatten()
                .into_iter()
                .collect(),
        })
        .collect();
    Segmented {
        segments,
        record_ids: Vec::new(),
    }
}

/// A conversation's topics as the step gives them, each with its messages' records.
fn conversation_topics(
    segments: &[ConversationSegment],
    records: &[TimelineRecord],
    messages: &[TranscriptLine],
) -> Segmented {
    let segments = segments
        .iter()
        .map(|segment| SegmentedTopic {
            title: segment.title.clone(),
            summary: segment.summary.clone(),
            start_ms: messages[segment.first].start_ms,
            end_ms: messages[segment.last].end_ms,
            projects: segment.projects.clone(),
            record_ids: records[segment.first..=segment.last]
                .iter()
                .map(|record| record.id)
                .collect(),
        })
        .collect();
    Segmented {
        segments,
        record_ids: records.iter().map(|record| record.id).collect(),
    }
}

/// The promises among `found` to record, each in the message of `records` (in the order the
/// answer numbered them) that it was made in: only those a follow-up can answer, through the
/// connector the message came from, since a follow-up is a reply to that message
/// (`docs/connectors.md`, Sending). Mail is answered from a mailbox the reader never sees the
/// person's own messages of, so a promise in it is never found there.
fn promises_to_keep(
    found: &[segmentation::FoundPromise],
    records: &[TimelineRecord],
) -> Vec<NewPromise> {
    let mut kept = Vec::new();
    for promise in found {
        let Some(record) = records.get(promise.message) else {
            continue;
        };
        let answered_through_its_connector = record.connector_id.is_some()
            && ConnectorKind::parse(&record.source_kind).is_some_and(|kind| {
                matches!(kinds::definition(kind).replies, Replies::Connector { .. })
            });
        if !answered_through_its_connector {
            continue;
        }
        kept.push(NewPromise {
            project_id: promise.project_id,
            timeline_record_id: record.id,
            recipient: promise.recipient.clone(),
            summary: promise.summary.clone(),
        });
    }
    kept
}

/// How long the meeting ran: what the source said, or where its transcript ends.
fn duration_of(current: &Meeting, transcript: &[TranscriptLine]) -> i64 {
    let spoken = transcript.iter().map(|line| line.end_ms).max().unwrap_or(0);
    current.duration_ms.unwrap_or(0).max(spoken)
}

/// A segment of `capture`'s transcript as a timeline record: its title, its transcript excerpt,
/// its speakers, and the span of the meeting it covers, with its clip when there is one. The
/// segment and its clip are timed on the capture's clock; the record is on the meeting's, the
/// capture's offset after it, as every time of a meeting is.
///
/// A record is unique per connector and the capture's id at its source, with the segment's index.
/// A capture with no id at its source, such as an upload, names its records by the meeting
/// (`meeting:<id>#<index>`), and with no connector the unique key cannot hold them once: what
/// does is that `publish` stores them under the meeting's lock, in the transaction that marks it
/// done, and stores nothing for a meeting done.
fn segment_record(
    current: &Meeting,
    capture: &MeetingCapture,
    transcript: &[TranscriptLine],
    index: usize,
    segment: &Segment,
    clip: Option<Clip>,
) -> NewTimelineRecord {
    let offset_ms = capture.offset_ms;
    let speakers = segmentation::speakers(transcript, segment).join(", ");
    let clip: Option<Value> = clip.map(|clip| {
        json!({
            "storageLocationId": clip.storage_location_id,
            "path": clip.path,
            "contentType": clip.content_type,
            "startMs": clip.start_ms + offset_ms,
            "endMs": clip.end_ms + offset_ms,
        })
    });
    NewTimelineRecord {
        id: Uuid::now_v7(),
        source_kind: SEGMENT_SOURCE_KIND.to_owned(),
        connector_id: capture.connector_id,
        external_id: Some(match &capture.external_id {
            Some(external_id) => format!("{external_id}#{index}"),
            None => format!("meeting:{}#{index}", current.id),
        }),
        occurred_at: current.started_at
            + chrono::Duration::milliseconds(segment.start_ms + offset_ms),
        actor: speakers.chars().take(ACTOR_MAX_CHARACTERS).collect(),
        title: segment.title.clone(),
        body: segmentation::excerpt(transcript, segment),
        url: capture.url.clone(),
        media: Some(json!({
            "meetingId": current.id,
            "startMs": segment.start_ms + offset_ms,
            "endMs": segment.end_ms + offset_ms,
            "clip": clip,
        })),
        metadata: json!({
            "meetingId": current.id,
            "meetingTitle": current.title,
            "meetingSource": capture.source_kind,
            "segmentIndex": index,
            "summary": segment.summary,
        }),
        created_by: current.created_by,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meetings::fetch::MeetingToFetch;
    use crate::meetings::segmentation::SegmentProject;
    use crate::models::meeting::tests::fetched_with;
    use crate::models::meeting_word::{self, Origin};
    use crate::models::project::NewProject;
    use crate::models::user::ELYSIA_USER_ID;
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    fn meeting() -> Meeting {
        Meeting {
            id: Uuid::now_v7(),
            title: "Weekly sync".to_owned(),
            started_at: "2026-10-21T15:00:00Z".parse().expect("a time"),
            duration_ms: Some(1_800_000),
            organizer: "ada@example.com".to_owned(),
            participants: json!(["ada@example.com"]),
            segments: None,
            state: "segmented".to_owned(),
            created_by: Uuid::now_v7(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            hold_for_approval: false,
        }
    }

    /// `current`'s capture through a Fireflies connector, `offset_ms` from the meeting's clock.
    fn capture(current: &Meeting, offset_ms: i64) -> MeetingCapture {
        MeetingCapture {
            id: Uuid::now_v7(),
            meeting_id: current.id,
            connector_id: Some(Uuid::now_v7()),
            source_kind: "fireflies".to_owned(),
            external_id: Some("01HX".to_owned()),
            url: Some("https://app.fireflies.ai/view/01HX".to_owned()),
            transcript: json!([]),
            transcript_hash: None,
            storage_location_id: None,
            recording_path: None,
            recording_content_type: None,
            recording_hash: None,
            offset_ms,
            created_by: current.created_by,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn line(start_ms: i64, speaker: &str, text: &str) -> TranscriptLine {
        TranscriptLine {
            start_ms,
            end_ms: start_ms + 3_000,
            speaker: speaker.to_owned(),
            text: text.to_owned(),
        }
    }

    #[test]
    fn every_input_segmenting_can_receive_reads_as_what_it_is() {
        let meeting_id = Uuid::now_v7();
        // What the correction gives, what the transcription gives when the correction is
        // skipped, and what the fetch took when everything before segmenting is skipped.
        let corrected = CorrectedMeeting {
            meeting_id,
            capture_id: Some(Uuid::now_v7()),
            transcript_hash: Some("a".repeat(64)),
            recording: None,
            whisper_transcript_hash: None,
            corrected_transcript_hash: Some("c".repeat(64)),
            approved: true,
        };
        let meetings = [
            serde_json::to_value(TranscriptToSegment::Meeting(corrected.clone()))
                .expect("serializes"),
            json!({ "meetingId": meeting_id, "transcriptHash": "a".repeat(64), "recording": null }),
            serde_json::to_value(MeetingToFetch { meeting_id }).expect("serializes"),
        ];
        assert_eq!(
            meetings[0],
            serde_json::to_value(&corrected).expect("serializes"),
            "a meeting to segment is the correction's own JSON"
        );
        for json in meetings {
            let read: TranscriptToSegment = serde_json::from_value(json).expect("reads");
            let TranscriptToSegment::Meeting(read) = read else {
                panic!("a meeting reads as a meeting, not {read:?}");
            };
            assert_eq!(read.meeting_id, meeting_id);
        }

        // What intake hands in when it enters the workflow here.
        let record_id = Uuid::now_v7();
        let conversation = json!({ "title": "#launch in Mooreslab", "recordIds": [record_id] });
        assert_eq!(
            serde_json::from_value::<TranscriptToSegment>(conversation).expect("reads"),
            TranscriptToSegment::Conversation(ConversationToSegment {
                title: "#launch in Mooreslab".to_owned(),
                record_ids: vec![record_id],
            })
        );
    }

    #[test]
    fn a_segment_becomes_a_record_at_its_time_with_its_excerpt_and_clip() {
        let current = meeting();
        let heard_in = capture(&current, 0);
        let transcript = vec![
            line(61_000, "Ada", "Launch is Friday."),
            line(400_000, "Grace", "Later topic."),
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
            &heard_in,
            &transcript,
            2,
            &segment,
            Some(Clip {
                storage_location_id: location,
                path: "meetings/x/segments/2.mp4".to_owned(),
                content_type: "video/mp4".to_owned(),
                start_ms: 60_400,
                end_ms: 299_700,
            }),
        );

        assert_eq!(record.source_kind, "meeting");
        assert_eq!(record.connector_id, heard_in.connector_id);
        assert_eq!(record.external_id.as_deref(), Some("01HX#2"));
        assert_eq!(record.url, heard_in.url);
        assert_eq!(record.metadata["meetingSource"], "fireflies");
        assert_eq!(record.occurred_at.to_rfc3339(), "2026-10-21T15:01:00+00:00");
        assert_eq!(record.actor, "Ada");
        assert_eq!(record.body, "[1:01] Ada: Launch is Friday.\n");
        assert_eq!(record.created_by, current.created_by);
        let media = record.media.expect("a span");
        assert_eq!(media["startMs"], 60_000);
        assert_eq!(media["clip"]["path"], "meetings/x/segments/2.mp4");
        assert_eq!(media["clip"]["storageLocationId"], json!(location));
        // The clip holds the stretch cut at Whisper's words; the span stays the transcript's.
        assert_eq!(
            (
                media["clip"]["startMs"].clone(),
                media["clip"]["endMs"].clone()
            ),
            (json!(60_400), json!(299_700))
        );
        assert_eq!(record.metadata["summary"], "Launch moves to Friday.");

        // A capture that began 90 seconds into the meeting places it 90 seconds later on the
        // meeting's clock, its clip with it.
        let later = capture(&current, 90_000);
        let placed = segment_record(
            &current,
            &later,
            &transcript,
            2,
            &segment,
            Some(Clip {
                storage_location_id: location,
                path: "meetings/x/segments/2.mp4".to_owned(),
                content_type: "video/mp4".to_owned(),
                start_ms: 60_400,
                end_ms: 299_700,
            }),
        );
        assert_eq!(placed.occurred_at.to_rfc3339(), "2026-10-21T15:02:30+00:00");
        let media = placed.media.expect("a span");
        assert_eq!(
            (media["startMs"].clone(), media["endMs"].clone()),
            (json!(150_000), json!(390_000))
        );
        assert_eq!(media["clip"]["startMs"], json!(150_400));
    }

    #[test]
    fn a_capture_with_no_id_at_its_source_names_its_records_by_the_meeting() {
        let current = meeting();
        let mut uploaded = capture(&current, 0);
        uploaded.connector_id = None;
        uploaded.external_id = None;
        let record = segment_record(
            &current,
            &uploaded,
            &[],
            3,
            &Segment {
                start_ms: 0,
                end_ms: 60_000,
                title: "Intros".to_owned(),
                summary: String::new(),
                projects: Vec::new(),
            },
            None,
        );
        assert_eq!(record.connector_id, None);
        assert_eq!(
            record.external_id,
            Some(format!("meeting:{}#3", current.id))
        );
    }

    #[test]
    fn a_segment_without_a_recording_still_names_its_span() {
        let current = meeting();
        let record = segment_record(
            &current,
            &capture(&current, 0),
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
        let spoken = TranscriptLine {
            start_ms: 0,
            end_ms: 2_000_000,
            speaker: String::new(),
            text: "x".to_owned(),
        };
        assert_eq!(
            duration_of(&current, std::slice::from_ref(&spoken)),
            2_000_000
        );
        current.duration_ms = None;
        assert_eq!(duration_of(&current, &[]), 0);
    }

    /// A meeting of the test person's, fetched with `transcript` and no recording.
    async fn fetched_meeting(
        connection: &mut AsyncPgConnection,
        transcript: &[TranscriptLine],
    ) -> (Meeting, MeetingCapture) {
        fetched_with(
            connection,
            serde_json::to_value(transcript).expect("serializes"),
        )
        .await
    }

    /// What a run whose correction was skipped hands segmenting for `fetched`.
    fn input_for((fetched, capture): &(Meeting, MeetingCapture)) -> TranscriptToSegment {
        TranscriptToSegment::Meeting(CorrectedMeeting {
            meeting_id: fetched.id,
            capture_id: Some(capture.id),
            transcript_hash: capture.transcript_hash.clone(),
            recording: None,
            whisper_transcript_hash: None,
            corrected_transcript_hash: None,
            approved: false,
        })
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    #[expect(
        clippy::too_many_lines,
        reason = "one meeting published, then published again, with what each left behind"
    )]
    async fn a_meetings_segments_are_published_once_attributed_by_elysia() {
        let (url, mut connection) = migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 4)
            .await
            .expect("a pool");
        let launch = project::create(
            &mut connection,
            &NewProject {
                name: "Launch".to_owned(),
                description: String::new(),
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("a project");
        let heard = fetched_meeting(
            &mut connection,
            &[
                line(61_000, "Ada", "Launch is Friday."),
                line(400_000, "Grace", "Lunch?"),
            ],
        )
        .await;
        let fetched = heard.0.clone();
        // An attempt before stored the answer, so this one asks the model nothing: the runner's
        // workspace never answers.
        let segments = vec![
            Segment {
                start_ms: 60_000,
                end_ms: 300_000,
                title: "Launch".to_owned(),
                summary: "Launch moves to Friday.".to_owned(),
                projects: vec![SegmentProject {
                    project_id: launch.id,
                    confidence: 0.8,
                }],
            },
            Segment {
                start_ms: 300_000,
                end_ms: 600_000,
                title: "Lunch".to_owned(),
                summary: String::new(),
                projects: Vec::new(),
            },
        ];
        meeting::record_segments(
            &mut connection,
            fetched.id,
            serde_json::to_value(&segments).expect("serializes"),
        )
        .await
        .expect("records")
        .expect("the first answer");

        let step = SegmentMeeting::for_test(database.clone());
        let done = step
            .run(
                input_for(&heard),
                StepContext::for_test::<SegmentMeeting>(database.clone()),
            )
            .await
            .expect("publishes");
        let Done::Succeeded {
            output: published,
            summary,
        } = done
        else {
            panic!("a segmented meeting is published, not {done:?}");
        };
        assert_eq!(
            summary.as_deref(),
            Some("Put 2 topics on the timeline, 1 of them on projects")
        );
        assert_eq!(published.segments.len(), 2);
        // Its correction was skipped, so its final transcript is built here, as its source heard
        // it.
        let words = meeting_word::standing(&mut connection, fetched.id)
            .await
            .expect("reads");
        assert_eq!(meeting_word::text_of(&words), "Launch is Friday. Lunch?");
        assert!(
            words
                .iter()
                .all(|word| word.origin == Origin::Source.as_str())
        );

        let records = timeline_record::of_meeting(&mut connection, TEST_PERSON_ID, fetched.id)
            .await
            .expect("reads");
        assert_eq!(records.len(), 2);
        let (launch_record, attributions) = records
            .iter()
            .find(|(record, _)| record.title == "Launch")
            .expect("the launch segment");
        assert_eq!(published.segments[0].record_ids, vec![launch_record.id]);
        assert_eq!(attributions.len(), 1);
        assert_eq!(
            (
                attributions[0].project_id,
                attributions[0].confidence,
                attributions[0].attributed_by.as_str()
            ),
            (launch.id, 0.8, "elysia")
        );
        let history = attribution::history(&mut connection, launch_record.id)
            .await
            .expect("history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].created_by, ELYSIA_USER_ID);
        assert_eq!(
            meeting::find(&mut connection, fetched.id)
                .await
                .expect("found")
                .state(),
            MeetingState::Done
        );

        // Run again, as a retried step or a later run would: nothing is published twice.
        let again = step
            .run(
                input_for(&heard),
                StepContext::for_test::<SegmentMeeting>(database),
            )
            .await
            .expect("answers");
        let Done::Succeeded {
            output: answered,
            summary,
        } = again
        else {
            panic!("a published meeting answers what it published, not {again:?}");
        };
        assert_eq!(
            summary.as_deref(),
            Some("Already on the timeline as 2 topics")
        );
        assert_eq!(answered, published);
        assert_eq!(
            timeline_record::of_meeting(&mut connection, TEST_PERSON_ID, fetched.id)
                .await
                .expect("reads")
                .len(),
            2
        );
    }

    /// A capture with no connector names its records by its meeting, which no unique key holds
    /// once, so two attempts segmenting it at once rest on the meeting's lock alone.
    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn two_attempts_at_once_publish_a_meeting_with_no_connector_once() {
        let (url, mut connection) = migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 4)
            .await
            .expect("a pool");
        let heard = fetched_meeting(
            &mut connection,
            &[
                line(61_000, "Ada", "Launch is Friday."),
                line(400_000, "Grace", "Lunch?"),
            ],
        )
        .await;
        assert_eq!(heard.1.connector_id, None);
        let segments = vec![
            Segment {
                start_ms: 60_000,
                end_ms: 300_000,
                title: "Launch".to_owned(),
                summary: String::new(),
                projects: Vec::new(),
            },
            Segment {
                start_ms: 300_000,
                end_ms: 600_000,
                title: "Lunch".to_owned(),
                summary: String::new(),
                projects: Vec::new(),
            },
        ];
        meeting::record_segments(
            &mut connection,
            heard.0.id,
            serde_json::to_value(&segments).expect("serializes"),
        )
        .await
        .expect("records")
        .expect("the first answer");

        let step = SegmentMeeting::for_test(database.clone());
        let (first, second) = tokio::join!(
            step.run(
                input_for(&heard),
                StepContext::for_test::<SegmentMeeting>(database.clone()),
            ),
            step.run(
                input_for(&heard),
                StepContext::for_test::<SegmentMeeting>(database.clone()),
            ),
        );
        let (Ok(Done::Succeeded { output: first, .. }), Ok(Done::Succeeded { output: second, .. })) =
            (first, second)
        else {
            panic!("both attempts succeed");
        };
        assert_eq!(first, second, "the later answers what the first published");
        let records = timeline_record::of_meeting(&mut connection, TEST_PERSON_ID, heard.0.id)
            .await
            .expect("reads");
        assert_eq!(records.len(), 2, "each segment is one record");
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_meeting_with_nothing_said_is_done_with_no_topics() {
        let (url, mut connection) = migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 4)
            .await
            .expect("a pool");
        let silent_heard = fetched_meeting(&mut connection, &[]).await;
        let silent = silent_heard.0.clone();

        let done = SegmentMeeting::for_test(database.clone())
            .run(
                input_for(&silent_heard),
                StepContext::for_test::<SegmentMeeting>(database),
            )
            .await
            .expect("skips");
        assert_eq!(
            done,
            Done::Skipped {
                reason: "Nothing was said, so nothing to segment".to_owned(),
            }
        );
        let stored = meeting::find(&mut connection, silent.id)
            .await
            .expect("found");
        assert_eq!(stored.state(), MeetingState::Done);
        assert_eq!(stored.segments, Some(json!([])));
    }
}
