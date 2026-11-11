// Copyright © 2026 Jalapeno Labs

//! The meeting workflow's third step, [`AlignMeeting`]: every capture of a meeting on its clock,
//! their words lined up into its final transcript, and a meeting that looks like another held
//! for its person to say whether it is. See `docs/meetings.md`, Aligning.
//!
//! # Placing each capture
//!
//! The first capture is the meeting's clock. Each other capture whose offset is still the one its
//! listing gave (`OffsetSource::Listed`) is aligned against it (`super::alignment`): by
//! cross-correlating the two recordings' audio when both have audio Whisper heard, and otherwise,
//! or when the audio gives no clear answer, by matching its words and their times against the
//! first capture's. One that neither places keeps its listed offset, and the log says so. An
//! offset found, or set by a person, is never found again.
//!
//! # Lining the transcript up
//!
//! Then the meeting's final transcript is lined up from every capture
//! (`super::final_transcript::rebuild`), keeping what did not change and every edit.
//!
//! # The same meeting twice
//!
//! Last, a meeting that overlaps another of its person's, with participants in common, and was
//! found after it, asks its person whether the two are one (`super::duplicates`), and the run
//! waits here ([`Done::Held`]) until they merge it or say it is not the same, so a duplicate
//! never publishes segments of its own. A meeting merged into another skips this step and every
//! one after it.
//!
//! The step also runs alone, as the meeting alignment workflow (`super::workflow::alignment`),
//! for a meeting whose captures changed after its own run aligned it: a merge, or a split.
//!
//! # What it logs
//!
//! Counts, offsets, and how each was found, never a title or a word that was said.

use std::sync::Arc;

use anyhow::anyhow;
use chrono::Utc;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use uuid::Uuid;

use super::alignment;
use super::correction;
use super::duplicates;
use super::final_transcript;
use super::media;
use super::transcribe::TranscribedMeeting;
use crate::agent_models::AgentKind;
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::agent::Author;
use crate::models::meeting::{self, Meeting, MeetingState};
use crate::models::meeting_capture::{self, MeetingCapture, OffsetSource};
use crate::models::meeting_transcription;
use crate::models::meeting_word::{BuiltWord, Origin};
use crate::models::user::SYSTEM_USER_ID;
use crate::realtime::EventBus;
use crate::routes::v1::action_items::publish_item_write;
use crate::runner::workspace::WorkspaceContainer;
use crate::storage::Storage;
use crate::workflows::{Done, Step, StepContext, StepFailure, StepLog};

/// Lines one meeting's captures up. See the module's docs.
#[derive(Clone)]
pub struct AlignMeeting {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    events: EventBus,
    cipher: Arc<Cipher>,
    storage: Storage,
    workspace: WorkspaceContainer,
}

impl std::fmt::Debug for AlignMeeting {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AlignMeeting")
            .finish_non_exhaustive()
    }
}

/// What [`AlignMeeting`] is built from.
#[derive(Clone)]
pub struct AlignMeetingDeps {
    pub database: Pool,
    pub events: EventBus,
    pub cipher: Arc<Cipher>,
    pub storage: Storage,
    pub workspace: WorkspaceContainer,
}

/// One capture as aligning reads it: its lines as it heard them, and Whisper's words when Whisper
/// heard it.
struct Heard {
    lines: Vec<super::source::TranscriptLine>,
    whisper: Vec<crate::whisper::WhisperWord>,
    /// Whether Whisper heard its recording, so its audio can be correlated.
    has_audio: bool,
}

impl Heard {
    /// Its words, timed on its own clock, `offset_ms` from the meeting's.
    fn words(&self, offset_ms: i64) -> Vec<BuiltWord> {
        let lines = self.lines.iter().map(|line| (line, Origin::Source));
        alignment::capture_words(lines, &self.whisper, offset_ms)
    }
}

impl AlignMeeting {
    pub fn new(deps: AlignMeetingDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                database: deps.database,
                events: deps.events,
                cipher: deps.cipher,
                storage: deps.storage,
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

    /// One attempt at aligning the meeting `input` names.
    async fn align(
        &self,
        input: TranscribedMeeting,
        log: &StepLog,
    ) -> Result<Done<TranscribedMeeting>, StepFailure> {
        if input.transcript_hash.is_none() {
            log.line("The meeting's fetch was skipped");
            return Ok(Done::Skipped {
                reason: "Not fetched, so nothing to align".to_owned(),
            });
        }
        let (current, captures) = {
            let mut connection = self.connection().await?;
            let current = meeting::find(&mut connection, input.meeting_id)
                .await
                .map_err(|error| match error {
                    diesel::result::Error::NotFound => {
                        StepFailure::permanent("The meeting no longer exists")
                    }
                    other => StepFailure::retryable(other),
                })?;
            let captures = meeting_capture::list_of(&mut connection, input.meeting_id)
                .await
                .map_err(StepFailure::retryable)?;
            (current, captures)
        };
        if current.merged_into.is_some() {
            log.line("The meeting's captures were merged into another meeting");
            return Ok(Done::Skipped {
                reason: "Merged into another meeting, so nothing to align".to_owned(),
            });
        }

        let fetched_media = self.place_captures(&current, &captures, log).await?;
        let words = self.line_up(current.id, log).await?;
        if fetched_media && current.state() == MeetingState::Done {
            // A meeting segmented already cuts no more clips, so nothing needs its recordings
            // in the workspace; one still on its way keeps them for segmenting.
            if let Err(error) = media::clean_up(&self.inner.workspace, current.id).await {
                log.line(&format!(
                    "The meeting's workspace directory could not be removed: {error}"
                ));
            }
        }

        let summary = if captures.len() == 1 {
            format!("{words} words in the transcript")
        } else {
            format!(
                "{words} words in the transcript, from {} captures",
                captures.len()
            )
        };
        if self.waits_for_its_person(current.id, log).await? {
            return Ok(Done::Held {
                output: input,
                summary: format!(
                    "{summary}; it looks like another meeting, waiting for its person"
                ),
            });
        }
        log.line(&summary);
        Ok(Done::Succeeded {
            output: input,
            summary: Some(summary),
        })
    }

    /// Places every capture of `current` (`captures`, in order) not placed yet on its clock, and
    /// answers whether any recording was fetched into the workspace to do so.
    async fn place_captures(
        &self,
        current: &Meeting,
        captures: &[MeetingCapture],
        log: &StepLog,
    ) -> Result<bool, StepFailure> {
        let Some((first, others)) = captures.split_first() else {
            return Ok(false);
        };
        // Numbered as the meeting orders them, the first being 1, so the log names each capture
        // the same way whichever of them were placed already.
        let unplaced: Vec<(usize, &MeetingCapture)> = (2..)
            .zip(others)
            .filter(|(_number, capture)| capture.offset_source() == OffsetSource::Listed)
            .collect();
        if unplaced.is_empty() {
            return Ok(false);
        }
        let reference = self.heard(first).await?;
        let reference_words = reference.words(first.offset_ms);
        // Read once, the first time a capture with audio needs it; `Some(None)` once it could not
        // be, after which no other recording is fetched to correlate against nothing.
        let mut reference_envelope: Option<Option<Arc<Vec<f32>>>> = None;
        let mut fetched_media = false;

        for (number, capture) in unplaced {
            let heard = self.heard(capture).await?;
            let mut placed = None;
            if reference.has_audio && heard.has_audio {
                fetched_media = true;
                if reference_envelope.is_none() {
                    let read = self.envelope(current.id, first, log).await?;
                    if read.is_none() {
                        log.line(
                            "The first capture's audio could not be read; every other capture is \
                             placed by its words",
                        );
                    }
                    reference_envelope = Some(read.map(Arc::new));
                }
                if let Some(Some(reference)) = &reference_envelope {
                    placed = self
                        .place_by_audio(current.id, capture, Arc::clone(reference), log)
                        .await?
                        .map(|lag| (first.offset_ms + lag, OffsetSource::Audio));
                    if placed.is_none() {
                        log.line(&format!(
                            "Capture {number}'s audio gives no clear lag against the first's; \
                             matching its words instead"
                        ));
                    }
                }
            }
            if placed.is_none() {
                placed = alignment::words_lag(&reference_words, &heard.words(0))
                    .map(|lag| (lag, OffsetSource::Words));
            }
            let Some((offset_ms, source)) = placed else {
                log.line(&format!(
                    "Capture {number} could not be placed by its audio or its words; it keeps the \
                     offset its listing gave, {} ms",
                    capture.offset_ms
                ));
                continue;
            };
            if self
                .set_offset(current.id, capture.id, offset_ms, source)
                .await?
            {
                log.line(&format!(
                    "Placed capture {number} at {offset_ms} ms on the meeting's clock, by its {}",
                    match source {
                        OffsetSource::Audio => "audio",
                        _ => "words",
                    }
                ));
            }
        }
        Ok(fetched_media)
    }

    /// How far `capture`'s audio runs behind the first capture's, whose envelope is `reference`;
    /// `None` when its own audio cannot be read or no lag stands out.
    async fn place_by_audio(
        &self,
        meeting_id: Uuid,
        capture: &MeetingCapture,
        reference: Arc<Vec<f32>>,
        log: &StepLog,
    ) -> Result<Option<i64>, StepFailure> {
        let Some(other) = self.envelope(meeting_id, capture, log).await? else {
            return Ok(None);
        };
        // Hundreds of thousands of frames each: arithmetic off the async runtime.
        tokio::task::spawn_blocking(move || alignment::audio_lag(&reference, &other))
            .await
            .map_err(|error| StepFailure::retryable(anyhow!("aligning stopped: {error}")))
    }

    /// What `capture` heard: its lines (its source's, or Whisper's segments when its source sent
    /// none), and Whisper's words.
    async fn heard(&self, capture: &MeetingCapture) -> Result<Heard, StepFailure> {
        let mut connection = self.connection().await?;
        let stored = meeting_transcription::find(&mut connection, capture.id)
            .await
            .map_err(StepFailure::retryable)?;
        let whisper: Option<crate::whisper::WhisperTranscript> = stored
            .map(|stored| serde_json::from_value(stored.transcript))
            .transpose()
            .map_err(|error| {
                StepFailure::permanent(format!(
                    "A capture's Whisper transcript is not in the stored shape: {error}"
                ))
            })?;
        let source: Vec<super::source::TranscriptLine> =
            serde_json::from_value(capture.transcript.clone()).map_err(|error| {
                StepFailure::permanent(format!(
                    "A capture's transcript is not in the stored shape: {error}"
                ))
            })?;
        let has_audio = whisper.is_some() && capture.stored_recording().is_some();
        let lines = correction::base(source, whisper.as_ref())
            .map(|base| base.lines)
            .unwrap_or_default();
        let whisper = whisper
            .into_iter()
            .flat_map(|heard| heard.segments)
            .flat_map(|segment| segment.words)
            .collect();
        Ok(Heard {
            lines,
            whisper,
            has_audio,
        })
    }

    /// `capture`'s energy envelope, from its recording fetched into meeting `meeting_id`'s
    /// workspace directory; `None` when `ffmpeg` cannot read its audio, which aligning then
    /// works around.
    async fn envelope(
        &self,
        meeting_id: Uuid,
        capture: &MeetingCapture,
        log: &StepLog,
    ) -> Result<Option<Vec<f32>>, StepFailure> {
        let Some(recording) = capture.stored_recording() else {
            return Ok(None);
        };
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
        let name = media::ensure_local(
            kept,
            &self.inner.workspace,
            (meeting_id, capture.id),
            &recording.path,
        )
        .await
        .map_err(StepFailure::retryable)?;
        match media::envelope(&self.inner.workspace, meeting_id, &name).await {
            Ok(envelope) => Ok(Some(envelope)),
            Err(media::MediaError::Failed(reason)) => {
                log.line(&format!("A recording's audio could not be read: {reason}"));
                Ok(None)
            }
            Err(error) => Err(StepFailure::retryable(error)),
        }
    }

    /// Sets capture `capture_id` of meeting `meeting_id` `offset_ms` from its clock as `source`
    /// found it, under the meeting's lock, unless a person or a merge moved it while it was being
    /// aligned. Answers whether it was set.
    async fn set_offset(
        &self,
        meeting_id: Uuid,
        capture_id: Uuid,
        offset_ms: i64,
        source: OffsetSource,
    ) -> Result<bool, StepFailure> {
        let mut connection = self.connection().await?;
        connection
            .transaction(async |connection| {
                meeting::lock(connection, meeting_id).await?;
                let capture = meeting_capture::find(connection, capture_id).await?;
                if capture.meeting_id != meeting_id
                    || capture.offset_source() != OffsetSource::Listed
                {
                    return Ok(false);
                }
                meeting_capture::set_offset(connection, capture_id, offset_ms, source).await?;
                diesel::QueryResult::Ok(true)
            })
            .await
            .map_err(StepFailure::retryable)
    }

    /// Lines meeting `meeting_id`'s transcript up from its captures, and answers how many words
    /// stand in it.
    async fn line_up(&self, meeting_id: Uuid, log: &StepLog) -> Result<usize, StepFailure> {
        let mut connection = self.connection().await?;
        let author = Author {
            user_id: SYSTEM_USER_ID,
            agent: None,
            at: Utc::now(),
        };
        let built = final_transcript::rebuild(&mut connection, meeting_id, author)
            .await
            .map_err(StepFailure::retryable)?;
        if let Some(built) = &built {
            log.line(&format!(
                "Lined the transcript up: {} words written, {} replaced, {} moved in time",
                built.data["added"], built.data["replaced"], built.data["retimed"]
            ));
        }
        let words = crate::models::meeting_word::standing_count(&mut connection, meeting_id)
            .await
            .map_err(StepFailure::retryable)?;
        Ok(words)
    }

    /// Whether meeting `meeting_id` waits for its person to say whether it is the same meeting as
    /// another, raising the inbox item that asks when it is the first to.
    async fn waits_for_its_person(
        &self,
        meeting_id: Uuid,
        log: &StepLog,
    ) -> Result<bool, StepFailure> {
        let mut connection = self.connection().await?;
        let asking = duplicates::ask_about(&mut connection, meeting_id)
            .await
            .map_err(StepFailure::retryable)?;
        if let Some(raised) = asking.raised {
            log.line("Raised an inbox item asking whether it is the same meeting as another");
            if let Err(error) =
                publish_item_write(&self.inner.events, &mut connection, raised, &[], Utc::now())
                    .await
            {
                // The item is stored; clients that missed it read it on their next load.
                tracing::event!(
                    name: "meetings.duplicates.unpublished",
                    tracing::Level::WARN,
                    meeting.id = %meeting_id,
                    error.message = %error,
                    "the inbox item asking about a duplicate was raised but could not be announced",
                );
            }
        }
        Ok(asking.waiting.is_some())
    }
}

impl Step for AlignMeeting {
    const NAME: &'static str = "align_meeting";
    /// Arithmetic over audio and words; no model of Claude's runs.
    const AGENT_KIND: Option<AgentKind> = None;
    type Input = TranscribedMeeting;
    type Output = TranscribedMeeting;

    async fn run(
        &self,
        input: TranscribedMeeting,
        context: StepContext,
    ) -> Result<Done<TranscribedMeeting>, StepFailure> {
        self.align(input, &context.log).await
    }
}

#[cfg(test)]
impl AlignMeeting {
    /// The step over `database`, with storage and a workspace that answer nothing: enough for
    /// meetings whose captures align by their words.
    pub fn for_test(database: Pool) -> Self {
        let docker = bollard::Docker::connect_with_http(
            "http://127.0.0.1:9",
            5,
            bollard::API_DEFAULT_VERSION,
        )
        .expect("a static address parses");
        Self::new(AlignMeetingDeps {
            database,
            events: EventBus::in_process(),
            cipher: Arc::new(crate::test_support::cipher()),
            storage: Storage::new(reqwest::Client::new()),
            workspace: WorkspaceContainer::new(docker, "elysium-workspace:test".to_owned()),
        })
    }
}
