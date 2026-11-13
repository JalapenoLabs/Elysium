// Copyright © 2026 Jalapeno Labs

//! The meeting workflow's last step, [`SegmentMeeting`]: what matters, taken out of what was said
//! and onto each project's timeline.
//!
//! It takes one of three things to segment ([`TranscriptToSegment`]):
//!
//! - **A meeting** past its correction, as the correction hands it on. Its final transcript, as
//!   its correction settled it and people edited it, is read in one of Elysia's text turns, which
//!   answers the excerpts it takes into projects and the black-outs it leaves out of everything,
//!   each a run of its lines (`super::segmentation`). Each excerpt becomes a mark over its words
//!   and a timeline record attributed to its projects, and each black-out a mark, in one
//!   transaction that also marks the meeting done (`super::excerpts::take`); then `ffmpeg` cuts
//!   each excerpt's clip from the meeting's recording in the workspace, at the words Whisper heard
//!   nearest its ends. A meeting whose aligning was skipped has its final transcript lined up here
//!   (`super::final_transcript`), so every meeting segmented has one. Done reviewing a meeting
//!   segments it again (`super::review`): every excerpt and black-out a person made or changed
//!   stands, and Elysia's are taken again, so only a project whose share changed hears of it.
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
//! What it gives ([`Segmented`]) names every topic's span, projects, and records (a meeting's
//! excerpts, a conversation's topics), for the steps after it, which read what landed on which
//! project.
//!
//! # Doing it once
//!
//! A meeting's answer is asked for once per segmenting: the first answer is stored on the meeting
//! (`meeting::record_segments`), its excerpts and black-outs as ranges of the words it read,
//! before anything is taken, and an attempt that finds it takes the same words without asking
//! again. The meeting is marked done in the transaction that takes them, so a meeting already done
//! answers what it took and changes nothing but the clips an attempt cut short left uncut. Done
//! reviewing clears the answer and the meeting's state in the transaction that starts its run, so
//! the run asks again. A conversation's segmentation is Elysia's whole reading of it: it replaces
//! what she said of its messages before, so reading it again, as it grows, never attributes a
//! message twice, and a message holds one promise at most, so it never records one twice either.
//!
//! # What it never overrides
//!
//! Every attribution goes through `models::attribution`, which records it in the record's
//! history and leaves alone whatever a person decided, so an excerpt a person moved stays moved.
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

use super::conversation;
use super::correct::CorrectedMeeting;
use super::excerpts::{self, Heading, MeetingText, Taken, TakenExcerpt, TakenTask, Took};
use super::final_transcript;
use super::media;
use super::segmentation::{
    self, ConversationSegment, LineRun, MeetingContext, PersonChoice, ProjectChoice, PromptLine,
};
use super::source::TranscriptLine;
use crate::action_items::WorkError;
use crate::agent_models::AgentKind;
use crate::connectors::ConnectorKind;
use crate::connectors::activity::Activity;
use crate::connectors::kinds::{self, Replies};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::agent::Author;
use crate::models::connector_run::ConnectorRun;
use crate::models::meeting::{self, Meeting, MeetingState};
use crate::models::meeting_capture::{self, MeetingCapture};
use crate::models::meeting_transcription;
use crate::models::meeting_word;
use crate::models::promise::{self, NewPromise, Promise};
use crate::models::timeline_record::{self, Attribution, TimelineRecord};
use crate::models::{project, user};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::routes::v1::action_items::publish_item_writes;
use crate::runner::events::{RunnerEvent, TurnStatus};
use crate::runner::workspace::WorkspaceContainer;
use crate::runner::{ConversationId, Runner, TurnMode, TurnRequest, WorkspacePath};
use crate::storage::Storage;
use crate::tools::grants::Grants;
use crate::tools::{self, CallScope};
use crate::whisper::{WhisperTranscript, WhisperWord};
use crate::workflows::{Done, Step, StepContext, StepFailure, StepLog};

/// Where segmentation turns run. They read and write no files, so the directory only needs to
/// exist.
const SEGMENTATION_DIRECTORY: &str = ".elysium/segmentation";

mod item;
mod pinning;

pub use pinning::TakenFrame;

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

/// One topic (a meeting's excerpt, a conversation's run of messages), and where it landed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentedTopic {
    pub title: String,
    pub summary: String,
    /// Its span: on the meeting's clock for an excerpt, from the first message for a
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
    grants: Grants,
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
    /// What a turn over a meeting's video is granted its frames tool with.
    pub grants: Grants,
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
                grants: deps.grants,
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

    /// The meeting `input` names and the capture it was corrected from, or `None` when its
    /// captures were merged into another meeting, which leaves it nothing to segment. A meeting
    /// its person deleted fails the step for good, stopping its run until they restore it.
    async fn meeting_and_capture(
        &self,
        input: &CorrectedMeeting,
    ) -> Result<Option<(Meeting, MeetingCapture)>, StepFailure> {
        let gone = |error| match error {
            diesel::result::Error::NotFound => {
                StepFailure::permanent("The meeting no longer exists")
            }
            other => StepFailure::retryable(other),
        };
        let mut connection = self.connection().await?;
        let current = meeting::find(&mut connection, input.meeting_id)
            .await
            .map_err(gone)?;
        if current.merged_into.is_some() {
            return Ok(None);
        }
        // A meeting its person deleted stops its run here until they restore it.
        if current.deleted_at.is_some() {
            return Err(StepFailure::permanent(super::deletion::STOPPED));
        }
        let capture = super::capture_of(&mut connection, input.meeting_id, input.capture_id)
            .await
            .map_err(gone)?;
        Ok(Some((current, capture)))
    }

    /// Takes what matters out of a meeting past its correction. See the module's docs.
    async fn segment_meeting(
        &self,
        input: CorrectedMeeting,
        context: &StepContext,
    ) -> Result<Done<Segmented>, StepFailure> {
        let log = &context.log;
        let Some((current, capture)) = self.meeting_and_capture(&input).await? else {
            log.line("The meeting's captures were merged into another meeting");
            return Ok(Done::Skipped {
                reason: "Merged into another meeting, so nothing to segment".to_owned(),
            });
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
        self.ensure_final_transcript(current.id, log).await?;
        let text = {
            let mut connection = self.connection().await?;
            MeetingText::load(&mut connection, current.id)
                .await
                .map_err(StepFailure::retryable)?
        };
        let taken = match self.stored_taken(&current, log).await? {
            Some(taken) => {
                log.line(&format!(
                    "Already read: {} excerpts, {} black-outs, and {} tasks; nothing is asked again",
                    taken.excerpts.len(),
                    taken.black_outs.len(),
                    taken.tasks.len()
                ));
                taken
            }
            None if text.words().is_empty() => return self.nothing_said(&current, log).await,
            None => {
                let asked = self.ask_meeting(&current, &text, context).await?;
                self.store_taken(current.id, asked, log).await?
            }
        };

        let frames = self.prepare_frames(&current, &taken.frames, log).await?;
        let published = self.publish(current.id, &taken, &frames).await?;
        self.forget_unpinned(&frames, log).await;
        let (took, pinned) = match published {
            Publishing::Took { took, pinned } => (*took, pinned),
            Publishing::ByAnother => return self.published_by_another(current.id, log).await,
            Publishing::Deleted => {
                // What it read is kept on the meeting, so restoring it takes the same excerpts.
                log.line("The meeting was deleted meanwhile, so nothing was taken");
                return Err(StepFailure::permanent(super::deletion::STOPPED));
            }
        };
        log_took(log, &taken, &took, pinned);
        self.announce_published(&current, took).await;
        let clipped = self.clip(&current, log).await?;
        let topics = self.topics(current.id).await?;
        if let Some(connector_id) = capture.connector_id {
            self.inner
                .activity
                .record(ConnectorRun::succeeded(
                    connector_id,
                    started,
                    topics.segments.len(),
                ))
                .await;
        }
        if clipped > 0
            && let Err(error) = media::clean_up(&self.inner.workspace, current.id).await
        {
            // The clips are up and the meeting is done; the scratch copy left behind costs
            // workspace space and nothing else, so the step still succeeds.
            log.line(&format!(
                "The meeting's workspace directory could not be removed: {error}"
            ));
        }
        Ok(Done::Succeeded {
            summary: Some(format!(
                "Took {} excerpts into projects, and blacked out {}",
                topics.segments.len(),
                taken.black_outs.len()
            )),
            output: topics,
        })
    }

    /// Lines meeting `meeting_id`'s final transcript up from its captures unless it has one: a
    /// meeting whose aligning a person skipped has none yet.
    async fn ensure_final_transcript(
        &self,
        meeting_id: Uuid,
        log: &StepLog,
    ) -> Result<(), StepFailure> {
        let mut connection = self.connection().await?;
        let has_words = meeting_word::has_words(&mut connection, meeting_id)
            .await
            .map_err(StepFailure::retryable)?;
        if has_words {
            return Ok(());
        }
        let author = Author {
            user_id: user::SYSTEM_USER_ID,
            agent: None,
            at: Utc::now(),
        };
        let built = final_transcript::rebuild(&mut connection, meeting_id, author)
            .await
            .map_err(StepFailure::retryable)?;
        if let Some(built) = built {
            log.line(&format!(
                "Lined the final transcript up: {} words",
                built.data["words"]
            ));
        }
        Ok(())
    }

    /// What segmenting read of `current` before, stored when an attempt asked: `None` when none
    /// did. A meeting cut into topics by a build before excerpts, and never published, has that
    /// answer cleared to be asked again.
    async fn stored_taken(
        &self,
        current: &Meeting,
        log: &StepLog,
    ) -> Result<Option<Taken>, StepFailure> {
        let Some(stored) = &current.segments else {
            return Ok(None);
        };
        if stored.is_array() {
            log.line("Its topics were stored before excerpts, so it is read again");
            let mut connection = self.connection().await?;
            meeting::reopen_for_segmenting(&mut connection, current.id)
                .await
                .map_err(StepFailure::retryable)?;
            return Ok(None);
        }
        serde_json::from_value(stored.clone())
            .map(Some)
            .map_err(|error| {
                StepFailure::permanent(format!(
                    "The meeting's excerpts are not in the stored shape: {error}"
                ))
            })
    }

    /// One segmentation turn over `text`, `current`'s transcript as it reads, its answer checked
    /// and its runs of lines turned into ranges of the words it read.
    async fn ask_meeting(
        &self,
        current: &Meeting,
        text: &MeetingText,
        context: &StepContext,
    ) -> Result<Taken, StepFailure> {
        let choices = self.project_choices().await?;
        let people = self.people_choices(current.created_by).await?;
        let words = text.words();
        let lines = text.lines();
        let timed = text.is_timed();
        let prompt_lines: Vec<PromptLine> = lines
            .iter()
            .map(|line| PromptLine {
                start_ms: words[line.start].start_ms,
                speaker: words[line.start].speaker.clone(),
                text: text.agent_text(line.clone()),
            })
            .collect();
        let participants: Vec<String> =
            serde_json::from_value(current.participants.clone()).unwrap_or_default();
        let meeting_context = MeetingContext {
            title: &current.title,
            duration_ms: duration_of(current, text),
            participants: &participants,
            person_id: current.created_by,
        };
        if !timed {
            context
                .log
                .line("The transcript carries no times, so its lines are read by number alone");
        }
        let prompt =
            segmentation::excerpts_prompt(meeting_context, &prompt_lines, timed, &choices, &people);
        let (answer, video) = self.ask_segmenting(current, prompt, timed, context).await?;
        let untitled = |first: usize| {
            if timed {
                format!(
                    "From {}",
                    segmentation::clock(words[lines[first].start].start_ms)
                )
            } else {
                format!("From line {}", first + 1)
            }
        };
        let reading =
            segmentation::parse_excerpts_answer(&answer, lines.len(), &choices, &people, untitled)
                .map_err(|error| {
                    StepFailure::retryable(format!("The segmentation's answer: {error}"))
                })?;
        context.log.line(&format!(
            "Read {} lines: {} excerpts, {} black-outs, {} tasks",
            lines.len(),
            reading.excerpts.len(),
            reading.black_outs.len(),
            reading.tasks.len()
        ));
        let range_of =
            |run: LineRun| text.range_of(&(lines[run.first].start..=lines[run.last].end - 1));
        let frames = taken_frames(&answer, video.as_ref(), lines.len(), range_of);
        if video.is_some() {
            context
                .log
                .line(&format!("It chose {} frames to pin", frames.len()));
        }
        Ok(Taken {
            excerpts: reading
                .excerpts
                .into_iter()
                .map(|found| TakenExcerpt {
                    range: range_of(found.lines),
                    heading: Heading {
                        title: found.title,
                        summary: found.summary,
                    },
                    projects: found.projects,
                })
                .collect(),
            black_outs: reading.black_outs.into_iter().map(range_of).collect(),
            tasks: reading
                .tasks
                .into_iter()
                .map(|found| TakenTask {
                    range: range_of(found.lines),
                    reading: found.reading,
                })
                .collect(),
            frames,
        })
    }

    /// Asks `current`'s segmentation turn `prompt`, offering it the meeting's video to pin frames
    /// of when it has one, and answers the turn's answer and the video it was offered. A meeting
    /// whose transcript is not `timed`, in words alone, has no clock to show its video by.
    async fn ask_segmenting(
        &self,
        current: &Meeting,
        mut prompt: String,
        timed: bool,
        context: &StepContext,
    ) -> Result<(String, Option<pinning::Video>), StepFailure> {
        let video = if timed {
            self.offer_video(current, &context.log).await?
        } else {
            None
        };
        let label = format!("meeting:{}:segmentation", current.id);
        let answer = match &video {
            None => {
                self.ask(label, prompt, segmentation::EXCERPTS_SYSTEM_PROMPT, context)
                    .await?
            }
            Some(video) => {
                prompt.push_str(&segmentation::frames_prompt(video.offered()));
                let system_prompt = format!(
                    "{} {}",
                    segmentation::EXCERPTS_SYSTEM_PROMPT,
                    segmentation::FRAMES_SYSTEM_PROMPT
                );
                self.ask_looking(label, prompt, system_prompt, video, context)
                    .await?
            }
        };
        Ok((answer, video))
    }

    /// Stores `asked` as what segmenting read of the meeting, or answers what another attempt
    /// stored first, which every later attempt takes.
    async fn store_taken(
        &self,
        meeting_id: Uuid,
        asked: Taken,
        log: &StepLog,
    ) -> Result<Taken, StepFailure> {
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
        log.line("Another attempt stored what it read first; it is kept");
        self.stored_taken(&stored, log)
            .await?
            .ok_or_else(|| StepFailure::retryable("What the meeting was read as went missing"))
    }

    /// How a meeting in which nothing was said ends: done, having taken nothing.
    async fn nothing_said(
        &self,
        current: &Meeting,
        log: &StepLog,
    ) -> Result<Done<Segmented>, StepFailure> {
        let mut connection = self.connection().await?;
        let meeting_id = current.id;
        let nothing = serde_json::to_value(Taken::default()).map_err(StepFailure::permanent)?;
        let done = connection
            .transaction(async move |connection| {
                meeting::record_segments(connection, meeting_id, nothing).await?;
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
    /// worked: with what that attempt took.
    async fn published_by_another(
        &self,
        meeting_id: Uuid,
        log: &StepLog,
    ) -> Result<Done<Segmented>, StepFailure> {
        log.line("Another attempt took the meeting's excerpts first; nothing was taken twice");
        let done = {
            let mut connection = self.connection().await?;
            meeting::find(&mut connection, meeting_id)
                .await
                .map_err(StepFailure::retryable)?
        };
        self.already_published(&done, log).await
    }

    /// What a meeting done already gives, changing nothing but the clips an attempt cut short
    /// left uncut.
    async fn already_published(
        &self,
        current: &Meeting,
        log: &StepLog,
    ) -> Result<Done<Segmented>, StepFailure> {
        let clipped = self.clip(current, log).await?;
        if clipped > 0
            && let Err(error) = media::clean_up(&self.inner.workspace, current.id).await
        {
            log.line(&format!(
                "The meeting's workspace directory could not be removed: {error}"
            ));
        }
        let topics = self.topics(current.id).await?;
        log.line(&format!(
            "Already took {} excerpts; nothing else was changed",
            topics.segments.len()
        ));
        Ok(Done::Succeeded {
            summary: Some(format!(
                "Already took {} excerpts into projects",
                topics.segments.len()
            )),
            output: topics,
        })
    }

    /// Meeting `meeting_id`'s standing excerpts as the step gives them, in the order they were
    /// said, each with its projects and its record.
    async fn topics(&self, meeting_id: Uuid) -> Result<Segmented, StepFailure> {
        let mut connection = self.connection().await?;
        let records = timeline_record::of_meeting(&mut connection, meeting_id)
            .await
            .map_err(StepFailure::retryable)?;
        Ok(excerpt_topics(records))
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

    /// Cuts and uploads the clip of each of `current`'s standing excerpts that has none, or whose
    /// words moved since its clip was cut, from the meeting's first capture with a recording kept,
    /// at the words Whisper heard nearest its ends; answers how many it cut. A meeting with no
    /// recording kept, such as one in words alone, has no clips.
    async fn clip(&self, current: &Meeting, log: &StepLog) -> Result<usize, StepFailure> {
        let (records, captures) = {
            let mut connection = self.connection().await?;
            let records = timeline_record::of_meeting(&mut connection, current.id)
                .await
                .map_err(StepFailure::retryable)?;
            let captures = meeting_capture::list_of(&mut connection, current.id)
                .await
                .map_err(StepFailure::retryable)?;
            (records, captures)
        };
        let Some(recording) = captures.into_iter().find_map(Recording::of) else {
            if !records.is_empty() {
                log.line("No recording is kept, so the excerpts carry no clips");
            }
            return Ok(0);
        };
        let words: Vec<WhisperWord> = self
            .whisper_transcript(&recording.capture)
            .await?
            .into_iter()
            .flat_map(|heard| heard.segments)
            .flat_map(|segment| segment.words)
            .collect();
        let uncut = uncut_clips(current.id, records, &recording, &words);
        if uncut.is_empty() {
            return Ok(0);
        }

        let (location, access_key) = {
            let mut connection = self.connection().await?;
            super::location(
                &mut connection,
                &self.inner.cipher,
                recording.storage_location_id,
            )
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
            (current.id, recording.capture.id),
            &recording.path,
        )
        .await
        .map_err(StepFailure::retryable)?;
        for clip in &uncut {
            self.cut_clip(kept, current.id, &recording_name, &recording, clip)
                .await?;
            log.line(&format!("Uploaded a clip to {}", clip.path));
        }
        if words.is_empty() {
            log.line(&format!("Cut {} clips at the words' times", uncut.len()));
        } else {
            log.line(&format!(
                "Cut {} clips at Whisper's word timings",
                uncut.len()
            ));
        }
        Ok(uncut.len())
    }

    /// Cuts `clip` from `recording`, the local copy of `from`, uploads it where `kept` says, and
    /// gives its record the clip.
    async fn cut_clip(
        &self,
        kept: media::Kept<'_>,
        meeting_id: Uuid,
        recording: &str,
        from: &Recording,
        clip: &Uncut,
    ) -> Result<(), StepFailure> {
        let clip_name = format!("excerpt-{}.{}", clip.record.id, from.extension());
        let (start_ms, end_ms) = clip.span;
        media::cut(
            &self.inner.workspace,
            meeting_id,
            recording,
            start_ms,
            end_ms,
            &clip_name,
        )
        .await
        .map_err(StepFailure::retryable)?;
        media::upload(
            kept,
            &self.inner.workspace,
            &media::directory(meeting_id),
            &clip_name,
            &clip.path,
            from.content_type(),
        )
        .await
        .map_err(StepFailure::retryable)?;
        // The clip's own stretch, on the meeting's clock as every time of a record is.
        let cut = json!({
            "storageLocationId": from.storage_location_id,
            "path": clip.path,
            "contentType": from.content_type(),
            "startMs": start_ms + from.capture.offset_ms,
            "endMs": end_ms + from.capture.offset_ms,
        });
        let mut connection = self.connection().await?;
        timeline_record::set_clip(&mut connection, clip.record.id, &cut)
            .await
            .map_err(StepFailure::retryable)
    }

    /// Takes `taken` into meeting `meeting_id` with its `frames` pinned, and marks it done, in one
    /// transaction under its lock, then refreshes what its excerpts and its own document read.
    /// Takes nothing when another attempt marked it done first, so two attempts at once take it
    /// once, nor when it was deleted meanwhile: the deletion takes the same lock, so no excerpt,
    /// task, or frame of a deleted meeting is ever taken.
    async fn publish(
        &self,
        meeting_id: Uuid,
        taken: &Taken,
        frames: &[pinning::Prepared],
    ) -> Result<Publishing, StepFailure> {
        let mut connection = self.connection().await?;
        connection
            .transaction(async move |connection| {
                let locked = meeting::lock(connection, meeting_id).await?;
                if locked.deleted_at.is_some() {
                    return Ok(Publishing::Deleted);
                }
                if locked.state() == MeetingState::Done {
                    return Ok(Publishing::ByAnother);
                }
                let capture = meeting_capture::first_of(connection, meeting_id).await?;
                let took = excerpts::take(connection, &locked, &capture, taken).await?;
                let pinned = pinning::pin_frames(connection, &locked, frames).await?;
                let done = meeting::record_done(connection, meeting_id).await?;
                excerpts::refresh(connection, &done, None).await?;
                Ok::<_, WorkError>(Publishing::Took {
                    took: Box::new(took),
                    pinned,
                })
            })
            .await
            .map_err(StepFailure::retryable)
    }

    /// Tells every client of each new excerpt's record, of each item its tasks raised, and of the
    /// meeting that finished, and the Project Managers that something may wait for them.
    async fn announce_published(&self, current: &Meeting, took: Took) {
        for record in &took.created {
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
            entry_id: None,
        });
        // Elysia's projects each kept or taken off an excerpt changed what it is attributed to, and
        // a task on a project waits for its Project Manager.
        self.inner.events.signal(&Signal::ProjectManagers);
        if took.raised.is_empty() {
            return;
        }
        let announced = match self.connection().await {
            Ok(mut connection) => publish_item_writes(
                &self.inner.events,
                &mut connection,
                took.raised,
                &[],
                Utc::now(),
            )
            .await
            .map(drop)
            .map_err(|error| format!("{error:?}")),
            Err(failure) => Err(format!("{failure:?}")),
        };
        if let Err(error) = announced {
            // The items are stored; a page that missed them reads them on its next load.
            tracing::event!(
                name: "meetings.segment.items_unannounced",
                tracing::Level::WARN,
                meeting.id = %current.id,
                error.message = %error,
                "the inbox items a meeting's tasks raised were stored but could not be announced",
            );
        }
    }

    fn announce_meeting(&self, changed: &Meeting) {
        self.inner.events.publish(&ServerEvent::MeetingChanged {
            id: changed.id,
            state: changed.state.clone(),
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

    /// Every active person on Elysium, as a task may be owed by them, and `person_id`, whose meeting
    /// it is, whatever their account's state: a task they took on is theirs.
    async fn people_choices(&self, person_id: Uuid) -> Result<Vec<PersonChoice>, StepFailure> {
        let mut connection = self.connection().await?;
        let users = user::list(&mut connection)
            .await
            .map_err(StepFailure::retryable)?;
        Ok(users
            .into_iter()
            .filter(|known| known.id == person_id || (known.is_active() && !known.is_machine()))
            .map(|person| PersonChoice {
                id: person.id,
                name: person.name,
                email: person.email,
            })
            .collect())
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
        let mode = TurnMode::Text {
            system_prompt: system_prompt.to_owned(),
        };
        self.ask_turn(label, prompt, mode, Vec::new(), context)
            .await
    }

    /// One turn over a meeting with `video`, which looks at its stills through the frames tool,
    /// granted over that video alone for the turn, and may hand them to subagents.
    async fn ask_looking(
        &self,
        label: String,
        prompt: String,
        system_prompt: String,
        video: &pinning::Video,
        context: &StepContext,
    ) -> Result<String, StepFailure> {
        let scope = CallScope::Segmenting(video.scope());
        let grant = self
            .inner
            .grants
            .grant(&scope, &[&tools::frames::SERVER])
            .await
            .map_err(StepFailure::retryable)?;
        let mode = TurnMode::TextWithSubagents { system_prompt };
        let answer = self
            .ask_turn(label, prompt, mode, grant.mcp_servers(), context)
            .await;
        grant.revoke().await;
        answer
    }

    /// One turn of `mode` with `mcp_servers` on the step's model, answering the model's final
    /// text.
    async fn ask_turn(
        &self,
        label: String,
        prompt: String,
        mode: TurnMode,
        mcp_servers: Vec<crate::runner::McpServer>,
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
            mcp_servers,
            allowed_tools: None,
            system_prompt_append: None,
            environment: Vec::new(),
            model: Some(model.model),
            effort: Some(model.effort),
            mode,
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

/// A workspace whose Docker answers nothing, for tests that never reach it.
#[cfg(test)]
fn unreachable_workspace() -> WorkspaceContainer {
    let docker =
        bollard::Docker::connect_with_http("http://127.0.0.1:9", 5, bollard::API_DEFAULT_VERSION)
            .expect("a static address parses");
    WorkspaceContainer::new(docker, "elysium-workspace:test".to_owned())
}

#[cfg(test)]
impl SegmentMeeting {
    /// The step over `database`, with a runner and a workspace Docker never answers: enough for
    /// a meeting segmented already, one not fetched, or one in which nothing was said.
    pub fn for_test(database: Pool) -> Self {
        let runner = Runner::new(
            unreachable_workspace(),
            database.clone(),
            Arc::new(crate::test_support::cipher()),
            crate::activity::Activities::new(),
            crate::claude_accounts::TurnSignal::default(),
            &tokio_util::sync::CancellationToken::new(),
        );
        Self::with_runner(database, runner)
    }

    /// The step as [`Self::for_test`] builds it, its segmentation turns answering `answers` in
    /// order.
    pub fn answering(database: Pool, answers: &[&str]) -> Self {
        Self::with_runner(database, Runner::answering(answers))
    }

    /// The step as [`Self::for_test`] builds it, over `storage`, opening locations sealed with
    /// `cipher`: for what it keeps in storage and deletes from it.
    pub fn with_storage(database: Pool, cipher: Arc<Cipher>, storage: Storage) -> Self {
        Self::with_parts(database, Runner::answering(&[]), cipher, storage)
    }

    fn with_runner(database: Pool, runner: Runner) -> Self {
        let cipher = Arc::new(crate::test_support::cipher());
        Self::with_parts(
            database,
            runner,
            cipher,
            Storage::new(reqwest::Client::new()),
        )
    }

    fn with_parts(database: Pool, runner: Runner, cipher: Arc<Cipher>, storage: Storage) -> Self {
        let workspace = unreachable_workspace();
        let database_for_grants = database.clone();
        let events = EventBus::in_process();
        Self::new(SegmentMeetingDeps {
            activity: Activity::new(database.clone(), events.clone()),
            database,
            events,
            cipher,
            storage,
            runner,
            workspace,
            grants: Grants::new(database_for_grants),
        })
    }
}

/// How taking a meeting's excerpts went, decided under its lock.
enum Publishing {
    /// This attempt took them, and pinned this many frames.
    Took { took: Box<Took>, pinned: usize },
    /// Another attempt marked the meeting done first, so nothing was taken twice.
    ByAnother,
    /// It was deleted meanwhile, so nothing was taken and the run stops.
    Deleted,
}

/// Every project's id now, so an attribution to one removed since the turn is left out rather
/// than failing the whole write.
async fn existing_projects(
    connection: &mut AsyncPgConnection,
) -> diesel::QueryResult<HashSet<Uuid>> {
    let projects = project::list(connection).await?;
    Ok(projects.into_iter().map(|project| project.id).collect())
}

/// The recording a meeting's clips are cut from: a capture's, kept in a storage location.
struct Recording {
    capture: MeetingCapture,
    storage_location_id: Uuid,
    path: String,
}

impl Recording {
    /// `capture`'s recording, when one is kept.
    fn of(capture: MeetingCapture) -> Option<Self> {
        let storage_location_id = capture.storage_location_id?;
        let path = capture.recording_path.clone()?;
        Some(Self {
            capture,
            storage_location_id,
            path,
        })
    }

    fn extension(&self) -> &str {
        media::extension_of(&self.path)
    }

    /// What its clips are, as its recording is.
    fn content_type(&self) -> &str {
        self.capture
            .recording_content_type
            .as_deref()
            .unwrap_or("application/octet-stream")
    }
}

/// A clip to cut: its excerpt's record, the stretch of the recording on its capture's clock, and
/// where it is kept.
struct Uncut {
    record: TimelineRecord,
    span: (i64, i64),
    path: String,
}

/// The clips of `records`, meeting `meeting_id`'s standing excerpts, not yet cut for the words each
/// holds now: one with no clip, and one whose words moved since, its clip's path naming another
/// stretch. Each is cut from `recording` at the Whisper `words` nearest its ends. An excerpt with no
/// span of its own, in a meeting in words alone, has no clip.
fn uncut_clips(
    meeting_id: Uuid,
    records: Vec<(TimelineRecord, Vec<Attribution>)>,
    recording: &Recording,
    words: &[WhisperWord],
) -> Vec<Uncut> {
    let offset_ms = recording.capture.offset_ms;
    let mut uncut = Vec::new();
    for (record, _attributions) in records {
        let Some(mark_id) = record.excerpt_mark_id else {
            continue;
        };
        let Some((_, start_ms, end_ms)) = timeline_record::segment_span(&record) else {
            continue;
        };
        if end_ms <= start_ms {
            continue;
        }
        // Excerpts are on the meeting's clock, and the recording on its capture's.
        let span = segmentation::clip_span(start_ms - offset_ms, end_ms - offset_ms, words);
        let path = media::excerpt_clip_path(meeting_id, mark_id, span, recording.extension());
        let cut_for = record
            .media
            .as_ref()
            .and_then(|media| media["clip"]["path"].as_str());
        if cut_for != Some(path.as_str()) {
            uncut.push(Uncut { record, span, path });
        }
    }
    uncut
}

/// The frames `answer` pins of `video`, each over the range of words its run of `line_count` lines
/// reads as; none for a meeting segmenting was offered no video of.
fn taken_frames(
    answer: &str,
    video: Option<&pinning::Video>,
    line_count: usize,
    range_of: impl Fn(LineRun) -> crate::models::meeting_mark::Range,
) -> Vec<TakenFrame> {
    let Some(video) = video else {
        return Vec::new();
    };
    segmentation::parse_frames_answer(answer, line_count, video.offered())
        .into_iter()
        .map(|found| TakenFrame {
            capture_id: video.capture.id,
            capture_ms: found.capture_ms,
            range: range_of(found.lines),
            caption: found.caption,
        })
        .collect()
}

/// A meeting's standing excerpts, `records` with their attributions, as the step gives them, in
/// the order they were said.
fn excerpt_topics(mut records: Vec<(TimelineRecord, Vec<Attribution>)>) -> Segmented {
    records.sort_by_key(|(record, _)| (record.occurred_at, record.id));
    let segments = records
        .into_iter()
        .map(|(record, attributions)| {
            let (start_ms, end_ms) = timeline_record::segment_span(&record)
                .map_or((0, 0), |(_, start_ms, end_ms)| (start_ms, end_ms));
            SegmentedTopic {
                summary: record.metadata["summary"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                title: record.title,
                start_ms,
                end_ms,
                projects: attributions
                    .into_iter()
                    .map(|attribution| segmentation::SegmentProject {
                        project_id: attribution.project_id,
                        confidence: attribution.confidence,
                    })
                    .collect(),
                record_ids: vec![record.id],
            }
        })
        .collect();
    Segmented {
        segments,
        record_ids: Vec::new(),
    }
}

/// Logs what publishing took of `taken`: the frames it pinned of those chosen, the marks it made,
/// kept, and removed, the inbox items its tasks raised, and the ranges edited while it read them.
fn log_took(log: &StepLog, taken: &Taken, took: &Took, pinned: usize) {
    if !taken.frames.is_empty() {
        log.line(&format!(
            "Pinned {pinned} of the {} frames it chose",
            taken.frames.len()
        ));
    }
    log.line(&format!(
        "Took {} excerpts, {} black-outs, and {} tasks: made {}, kept {} of Elysia's own, and \
         removed {}",
        taken.excerpts.len(),
        taken.black_outs.len(),
        taken.tasks.len(),
        count_in(&took.entry.data, "made"),
        count_in(&took.entry.data, "kept"),
        count_in(&took.entry.data, "removed"),
    ));
    if !took.raised.is_empty() {
        log.line(&format!(
            "Its tasks on no project raised {} inbox items",
            took.raised.len()
        ));
    }
    let unanchored = took.entry.data["unanchored"].as_u64().unwrap_or(0);
    if unanchored > 0 {
        log.line(&format!(
            "{unanchored} of its ranges were edited while it read them, so they were left out"
        ));
    }
}

/// How many ids a `segmented` history entry lists at `key`.
fn count_in(data: &Value, key: &str) -> usize {
    data[key].as_array().map_or(0, Vec::len)
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

/// How long the meeting ran: what its source said, or where its transcript ends.
fn duration_of(current: &Meeting, text: &MeetingText) -> i64 {
    let spoken = text
        .words()
        .iter()
        .map(|word| word.end_ms)
        .max()
        .unwrap_or(0);
    current.duration_ms.unwrap_or(0).max(spoken)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meetings::fetch::MeetingToFetch;
    use crate::models::meeting::tests::fetched_with;
    use crate::models::meeting_mark::{self, Mark, MarkKind};
    use crate::models::meeting_word::Origin;
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
            merged_into: None,
            visibility: "public".to_owned(),
            visibility_item_id: None,
            deleted_at: None,
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
            serde_json::to_value(MeetingToFetch {
                meeting_id,
                capture_id: None,
            })
            .expect("serializes"),
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
    fn a_meeting_runs_as_long_as_its_source_says_or_its_transcript_does() {
        let mut current = meeting();
        let nothing = MeetingText::new(Vec::new(), &[]);
        assert_eq!(duration_of(&current, &nothing), 1_800_000);
        current.duration_ms = None;
        assert_eq!(duration_of(&current, &nothing), 0);
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

    async fn project_named(connection: &mut AsyncPgConnection, name: &str) -> project::Project {
        project::create(
            connection,
            &NewProject {
                name: name.to_owned(),
                description: String::new(),
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("a project")
    }

    /// How many times project `project_id`'s Project Manager was woken.
    async fn wakes_of(connection: &mut AsyncPgConnection, project_id: Uuid) -> i64 {
        use crate::database::schema::project_manager_wakes;
        use diesel::{ExpressionMethods, QueryDsl};
        use diesel_async::RunQueryDsl;

        project_manager_wakes::table
            .filter(project_manager_wakes::project_id.eq(project_id))
            .count()
            .get_result(connection)
            .await
            .expect("counts")
    }

    /// Meeting `meeting_id`'s whole transcript as the knowledge index holds it.
    async fn indexed_transcript(connection: &mut AsyncPgConnection, meeting_id: Uuid) -> String {
        use crate::database::schema::{knowledge_documents, knowledge_passages};
        use diesel::{ExpressionMethods, QueryDsl};
        use diesel_async::RunQueryDsl;

        let passages: Vec<String> = knowledge_passages::table
            .inner_join(knowledge_documents::table)
            .filter(knowledge_documents::meeting_id.eq(meeting_id))
            .order(knowledge_passages::position.asc())
            .select(knowledge_passages::content)
            .load(connection)
            .await
            .expect("reads");
        passages.join("\n")
    }

    async fn standing_of(
        connection: &mut AsyncPgConnection,
        meeting_id: Uuid,
        kind: MarkKind,
    ) -> Vec<Mark> {
        meeting_mark::standing(connection, meeting_id)
            .await
            .expect("reads")
            .into_iter()
            .filter(|mark| mark.kind() == Some(kind))
            .collect()
    }

    fn excerpt(first: usize, last: usize, title: &str, projects: &[(Uuid, f32)]) -> Value {
        json!({
            "firstLine": first,
            "lastLine": last,
            "title": title,
            "summary": format!("{title}, said and decided."),
            "projects": projects
                .iter()
                .map(|(project_id, confidence)| json!({
                    "projectId": project_id,
                    "confidence": confidence,
                }))
                .collect::<Vec<_>>(),
        })
    }

    fn answer(excerpts: &[Value], black_outs: &[(usize, usize)]) -> String {
        json!({
            "excerpts": excerpts,
            "blackOuts": black_outs
                .iter()
                .map(|(first, last)| json!({ "firstLine": first, "lastLine": last }))
                .collect::<Vec<_>>(),
        })
        .to_string()
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    #[expect(
        clippy::too_many_lines,
        reason = "one meeting taken, then taken again, with what each left behind"
    )]
    async fn a_meetings_excerpts_and_black_outs_are_taken_once_attributed_by_elysia() {
        let (url, mut connection) = migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 4)
            .await
            .expect("a pool");
        let launch = project_named(&mut connection, "Launch").await;
        let heard = fetched_meeting(
            &mut connection,
            &[
                line(0, "Ada", "Is my mic on?"),
                line(5_000, "Grace", "Yes we hear you."),
                line(61_000, "Ada", "Launch is Friday."),
                line(400_000, "Grace", "Lunch?"),
            ],
        )
        .await;
        let fetched = heard.0.clone();
        let read = answer(&[excerpt(3, 3, "Launch", &[(launch.id, 0.8)])], &[(1, 2)]);
        let step = SegmentMeeting::answering(database.clone(), &[&read]);

        let done = step
            .run(
                input_for(&heard),
                StepContext::for_test::<SegmentMeeting>(database.clone()),
            )
            .await
            .expect("takes");
        let Done::Succeeded {
            output: taken,
            summary,
        } = done
        else {
            panic!("a segmented meeting is taken, not {done:?}");
        };
        assert_eq!(
            summary.as_deref(),
            Some("Took 1 excerpts into projects, and blacked out 1")
        );
        // Its correction was skipped, so its final transcript is built here, as its source heard
        // it.
        let words = meeting_word::standing(&mut connection, fetched.id)
            .await
            .expect("reads");
        assert_eq!(
            meeting_word::text_of(&words),
            "Is my mic on? Yes we hear you. Launch is Friday. Lunch?"
        );
        assert!(
            words
                .iter()
                .all(|word| word.origin == Origin::Source.as_str())
        );

        let records = timeline_record::of_meeting(&mut connection, fetched.id)
            .await
            .expect("reads");
        assert_eq!(records.len(), 1, "small talk is in no excerpt");
        let (record, attributions) = &records[0];
        assert_eq!(taken.segments.len(), 1);
        assert_eq!(taken.segments[0].record_ids, vec![record.id]);
        assert_eq!(taken.segments[0].summary, "Launch, said and decided.");
        assert_eq!(record.title, "Launch");
        assert_eq!(record.body, "[1:01] Ada: Launch is Friday.\n");
        assert_eq!(record.actor, "Ada");
        assert_eq!(
            record.occurred_at,
            fetched.started_at + chrono::Duration::milliseconds(61_000)
        );
        assert_eq!(record.metadata["meetingId"], json!(fetched.id));
        assert!(
            record.media.as_ref().expect("a span")["clip"].is_null(),
            "no recording is kept, so no clip"
        );
        assert_eq!(
            (
                attributions[0].project_id,
                attributions[0].confidence,
                attributions[0].attributed_by.as_str()
            ),
            (launch.id, 0.8, "elysia")
        );
        assert_eq!(wakes_of(&mut connection, launch.id).await, 1);

        let excerpts = standing_of(&mut connection, fetched.id, MarkKind::Excerpt).await;
        assert_eq!(excerpts.len(), 1);
        assert_eq!(record.excerpt_mark_id, Some(excerpts[0].id));
        assert_eq!(excerpts[0].created_by, ELYSIA_USER_ID);
        let black_outs = standing_of(&mut connection, fetched.id, MarkKind::BlackOut).await;
        assert_eq!(black_outs.len(), 1);
        assert_eq!(
            (black_outs[0].start_word_id, black_outs[0].end_word_id),
            (words[0].id, words[7].id),
            "the hellos are blacked out, word for word"
        );

        // The meeting's own document holds every word but the blacked-out ones.
        let indexed = indexed_transcript(&mut connection, fetched.id).await;
        assert!(indexed.contains("Launch is Friday."), "{indexed}");
        assert!(indexed.contains("Lunch?"), "{indexed}");
        assert!(!indexed.contains("mic"), "{indexed}");
        assert!(!indexed.contains("hear you"), "{indexed}");

        let published = meeting::find(&mut connection, fetched.id)
            .await
            .expect("found");
        assert_eq!(published.state(), MeetingState::Done);

        // Run again, as a retried step would: nothing is asked or taken twice.
        let again = SegmentMeeting::for_test(database.clone())
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
            panic!("a taken meeting answers what it took, not {again:?}");
        };
        assert_eq!(
            summary.as_deref(),
            Some("Already took 1 excerpts into projects")
        );
        assert_eq!(answered, taken);
        assert_eq!(wakes_of(&mut connection, launch.id).await, 1);
    }

    /// #241: Done reviewing segments a meeting again. Every excerpt and black-out a person made or
    /// changed stands, Elysia's own are taken again or removed, and only a project an excerpt is
    /// newly on hears of it.
    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    #[expect(
        clippy::too_many_lines,
        reason = "a meeting taken, changed by its person, and taken again"
    )]
    async fn segmenting_again_keeps_what_people_decided_and_wakes_only_a_changed_share() {
        let (url, mut connection) = migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 4)
            .await
            .expect("a pool");
        let launch = project_named(&mut connection, "Launch").await;
        let budget = project_named(&mut connection, "Budget").await;
        let hiring = project_named(&mut connection, "Hiring").await;
        let heard = fetched_meeting(
            &mut connection,
            &[
                line(0, "Ada", "Is my mic on?"),
                line(61_000, "Grace", "Launch is Friday."),
                line(120_000, "Ada", "The budget doubles."),
                line(180_000, "Grace", "We hire two people."),
                line(240_000, "Ada", "Lunch?"),
            ],
        )
        .await;
        let fetched = heard.0.clone();
        let first = answer(
            &[
                excerpt(2, 2, "Launch", &[(launch.id, 0.8)]),
                excerpt(3, 3, "Budget", &[(budget.id, 0.7)]),
                excerpt(4, 4, "Hiring", &[(hiring.id, 0.6)]),
            ],
            &[(1, 1)],
        );
        SegmentMeeting::answering(database.clone(), &[&first])
            .run(
                input_for(&heard),
                StepContext::for_test::<SegmentMeeting>(database.clone()),
            )
            .await
            .expect("takes");
        let by_title = |records: &[(TimelineRecord, Vec<Attribution>)], title: &str| {
            let (record, attributions) = records
                .iter()
                .find(|(record, _)| record.title == title)
                .unwrap_or_else(|| panic!("an excerpt titled {title}"));
            (record.clone(), attributions.clone())
        };
        let records = timeline_record::of_meeting(&mut connection, fetched.id)
            .await
            .expect("reads");
        let (launch_record, _) = by_title(&records, "Launch");
        let (budget_record, _) = by_title(&records, "Budget");
        let (hiring_record, _) = by_title(&records, "Hiring");

        // Its person reviews it: takes Elysia's budget excerpt out, puts the hiring one on the
        // budget too, and takes the last line into the launch themselves.
        let person = Author {
            user_id: TEST_PERSON_ID,
            agent: None,
            at: Utc::now(),
        };
        let published = meeting::find(&mut connection, fetched.id)
            .await
            .expect("found");
        excerpts::remove(
            &mut connection,
            &published,
            budget_record.excerpt_mark_id.expect("an excerpt"),
            person,
        )
        .await
        .expect("removes");
        crate::models::attribution::attribute(
            &mut connection,
            hiring_record.id,
            budget.id,
            1.0,
            crate::models::attribution::Attributor::Person(TEST_PERSON_ID),
        )
        .await
        .expect("attributes");
        let words = meeting_word::standing(&mut connection, fetched.id)
            .await
            .expect("reads");
        let last_word = words.last().expect("words");
        let persons = excerpts::assign(
            &mut connection,
            &published,
            crate::models::meeting_mark::Range {
                start_word_id: last_word.id,
                end_word_id: last_word.id,
            },
            &[segmentation::SegmentProject {
                project_id: launch.id,
                confidence: 1.0,
            }],
            person,
        )
        .await
        .expect("assigns");
        assert_eq!(wakes_of(&mut connection, launch.id).await, 2);
        assert_eq!(wakes_of(&mut connection, budget.id).await, 2);
        assert_eq!(wakes_of(&mut connection, hiring.id).await, 1);

        // Done reviewing, as `review::done_reviewing` does it, and the run that follows.
        meeting::reopen_for_segmenting(&mut connection, fetched.id)
            .await
            .expect("reopens");
        let second = answer(
            &[
                // The same launch excerpt, surer.
                excerpt(2, 2, "Launch moves", &[(launch.id, 0.9)]),
                // The one its person took out.
                excerpt(3, 3, "Budget", &[(budget.id, 0.7)]),
                // The one its person changed.
                excerpt(4, 4, "Hiring", &[(hiring.id, 0.4)]),
                // A new one, around two lines.
                excerpt(2, 3, "Launch costs", &[(launch.id, 0.7)]),
            ],
            &[],
        );
        let done = SegmentMeeting::answering(database.clone(), &[&second])
            .run(
                input_for(&heard),
                StepContext::for_test::<SegmentMeeting>(database.clone()),
            )
            .await
            .expect("takes again");
        assert!(matches!(done, Done::Succeeded { .. }), "{done:?}");

        let records = timeline_record::of_meeting(&mut connection, fetched.id)
            .await
            .expect("reads");
        let mut titles: Vec<&str> = records
            .iter()
            .map(|(record, _)| record.title.as_str())
            .collect();
        titles.sort_unstable();
        assert_eq!(
            titles,
            [
                "Hiring",
                "Launch costs",
                "Launch moves",
                persons.record.title.as_str()
            ],
            "Elysia's launch excerpt is kept and renamed, the one taken out stays out, and both the \
             changed one and the person's own stand"
        );
        let (kept, kept_attributions) = by_title(&records, "Launch moves");
        assert_eq!(kept.id, launch_record.id, "kept as it was, not made again");
        assert!((kept_attributions[0].confidence - 0.9).abs() < f32::EPSILON);
        let (changed, changed_attributions) = by_title(&records, "Hiring");
        assert_eq!(changed.id, hiring_record.id);
        assert_eq!(
            changed_attributions.len(),
            2,
            "the person's project stands with hers"
        );
        // Only the new excerpt woke the launch's Project Manager; the kept one did not again.
        assert_eq!(wakes_of(&mut connection, launch.id).await, 3);
        assert_eq!(wakes_of(&mut connection, budget.id).await, 2);
        assert_eq!(wakes_of(&mut connection, hiring.id).await, 1);
        assert!(
            standing_of(&mut connection, fetched.id, MarkKind::BlackOut)
                .await
                .is_empty(),
            "Elysia's own black-out, not taken again, is removed"
        );
    }

    /// A capture with no connector names its excerpts by their marks, so two attempts segmenting
    /// it at once rest on the meeting's lock alone.
    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn two_attempts_at_once_take_a_meeting_once() {
        let (url, mut connection) = migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 4)
            .await
            .expect("a pool");
        let launch = project_named(&mut connection, "Launch").await;
        let heard = fetched_meeting(
            &mut connection,
            &[
                line(61_000, "Ada", "Launch is Friday."),
                line(400_000, "Grace", "Lunch?"),
            ],
        )
        .await;
        assert_eq!(heard.1.connector_id, None);
        let read = answer(&[excerpt(1, 1, "Launch", &[(launch.id, 0.8)])], &[(2, 2)]);

        let step = SegmentMeeting::answering(database.clone(), &[&read, &read]);
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
        assert_eq!(first, second, "the later answers what the first took");
        let records = timeline_record::of_meeting(&mut connection, heard.0.id)
            .await
            .expect("reads");
        assert_eq!(records.len(), 1, "the excerpt is one record");
        assert_eq!(
            standing_of(&mut connection, heard.0.id, MarkKind::BlackOut)
                .await
                .len(),
            1
        );
    }

    /// #233: a transcript uploaded with no times and no recording is a meeting in words alone,
    /// read by its numbered lines alone, its excerpts with no times and no clip.
    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_meeting_in_words_alone_is_read_by_its_lines() {
        let (url, mut connection) = migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 4)
            .await
            .expect("a pool");
        let launch = project_named(&mut connection, "Launch").await;
        let untimed = |speaker: &str, text: &str| TranscriptLine {
            start_ms: 0,
            end_ms: 0,
            speaker: speaker.to_owned(),
            text: text.to_owned(),
        };
        let heard = fetched_meeting(
            &mut connection,
            &[
                untimed("Ada", "Hello everyone."),
                untimed("Ada", "Launch is Friday."),
                untimed("Grace", "How was the weekend?"),
            ],
        )
        .await;
        // One speaker with no pause reads as one line, as correction reads it.
        let read = answer(&[excerpt(1, 1, "Launch", &[(launch.id, 0.9)])], &[(2, 2)]);
        let step = SegmentMeeting::answering(database.clone(), &[&read]);

        let done = step
            .run(
                input_for(&heard),
                StepContext::for_test::<SegmentMeeting>(database.clone()),
            )
            .await
            .expect("an attempt");

        assert!(matches!(done, Done::Succeeded { .. }), "{done:?}");
        let records = timeline_record::of_meeting(&mut connection, heard.0.id)
            .await
            .expect("reads");
        let (record, _attributions) = &records[0];
        assert_eq!(record.body, "Ada: Hello everyone. Launch is Friday.\n");
        let media = record.media.as_ref().expect("a span");
        assert_eq!((&media["startMs"], &media["endMs"]), (&json!(0), &json!(0)));
        assert_eq!(media["clip"], Value::Null);
        let indexed = indexed_transcript(&mut connection, heard.0.id).await;
        assert!(!indexed.contains("weekend"), "{indexed}");
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_meeting_with_nothing_said_is_done_having_taken_nothing() {
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
        assert_eq!(
            stored.segments,
            Some(serde_json::to_value(Taken::default()).expect("serializes"))
        );
    }
}
