// Copyright © 2026 Jalapeno Labs

//! The meeting workflow's correction step, [`CorrectTranscript`]: one correcting agent settles the
//! meeting's final transcript, reading every capture's version of it beside the Transcription doc
//! and the company doc, and the corrected copy of its first capture's lines is stored beside the
//! raw transcripts it was made from.
//!
//! The prompt and the checks on the model's answer are `super::correction`; this module reads the
//! meeting, runs the turn, settles the transcript (`meeting_word::settle`) and stores the copy in
//! one transaction, teaches the Transcription doc (`crate::transcription_doc`), and decides whether
//! the run waits for its person's approval.
//!
//! # Doing it once
//!
//! A meeting is settled once. The raw transcripts are never written: the copy goes in
//! `meeting_corrections` (`models::meeting_correction`) in the transaction that settles the final
//! transcript, under the meeting's lock, so an attempt that finds it answers it without asking the
//! model anything, and an attempt that finds another stored first discards its own. A rebuild keeps
//! what the settlement wrote, so a merge or a split lines the transcript up again without settling
//! it again.
//!
//! # Teaching the Transcription doc
//!
//! A flag the agent is sure of ([`TEACHING_CONFIDENCE`] or more) that fixes words teaches the
//! Transcription doc each of its fixes, with where it was heard and why, in the same transaction,
//! so the next meeting's correction reads it at once; a fix naming someone or something the
//! company doc does not know is suggested to it.
//!
//! # Waiting for approval
//!
//! Approving a correction is optional, unless the meeting's source has "Wait for my approval
//! before acting on meetings" on (`super::waits_for_approval`) or its person held the meeting by
//! hand (`meetings.hold_for_approval`). Then a copy not yet approved holds the run
//! ([`Done::Held`]), with an inbox item asking for the review, raised in the same transaction
//! that records it on the copy, so an attempt that runs again holds without raising a second.
//! Approving it (`approve`) resumes the run with the step's output saying so. Resuming it from
//! the Runs page instead lets the run go on with the copy unapproved, and leaves the item for
//! its person to resolve.
//!
//! # What it logs
//!
//! Every company member can read a run's log, and meetings are their owner's alone, so the log
//! carries counts, line numbers, and hashes, never a word of what was said.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use chrono::Utc;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::correction::{self, Background, Base, Changed, Flag, Transcript, Version};
use super::final_transcript;
use super::segment::TranscriptToSegment;
use super::source::TranscriptLine;
use super::transcribe::TranscribedMeeting;
use crate::action_items::{Actor, Transition, WorkError};
use crate::agent_models::AgentKind;
use crate::company_doc;
use crate::database::Pool;
use crate::models::action_item::{
    self, ActionItem, ActionItemPriority, ActionItemState, NewActionItem, Owner,
};
use crate::models::action_item_event::Recorded;
use crate::models::agent::Author;
use crate::models::connector;
use crate::models::meeting::{self, Meeting};
use crate::models::meeting_capture::{self, StoredRecording};
use crate::models::meeting_correction::{self, MeetingCorrection, NewMeetingCorrection};
use crate::models::meeting_event::{self, Change, EventKind, MeetingEvent};
use crate::models::meeting_word::{self, Origin, SettledRange, Word};
use crate::models::model_setting::ModelChoice;
use crate::models::page_suggestion::Suggested;
use crate::models::step_job::{self, StepActionError, StepJobState};
use crate::models::user::{ELYSIA_USER_ID, SYSTEM_USER_ID};
use crate::models::workflow_run::{self, Settled};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::routes::v1::action_items::publish_item_write;
use crate::routes::v1::meetings::announce_lined_up;
use crate::routes::v1::pages;
use crate::runner::events::{RunnerEvent, TurnOutcome, TurnStatus};
use crate::runner::{ConversationId, Runner, TurnMode, TurnRequest, WorkspacePath};
use crate::transcription_doc::{self, Lesson, MeetingLink};
use crate::whisper::WhisperWord;
use crate::workflows::{self, Done, Step, StepContext, StepFailure, StepLog};

/// Where correction turns run in the workspace. Text turns read and write no files.
const CORRECTION_DIRECTORY: &str = ".elysium/correction";

/// How many of the company's newest meetings a correction reads speakers' names from: enough to
/// know the company's recurring names, few enough to read in one query.
const OTHER_MEETINGS: i32 = 30;

/// The most speakers' names a prompt lists.
const SPEAKERS_MAX: i32 = 200;

/// The longest title of an inbox item, as items allow.
const ITEM_TITLE_MAX_CHARACTERS: usize = 500;

/// How sure the correcting agent must be of a fix for the Transcription doc to learn it: sure
/// enough that it would be wrong rarely, since every later meeting reads what it learns, and a
/// person's own fix teaches it whatever the agent thought.
pub const TEACHING_CONFIDENCE: u8 = 80;

/// A meeting past its correction: everything the transcription gave, and which corrected copy
/// the meeting has. [`CorrectTranscript`] gives it as segmenting's input
/// ([`TranscriptToSegment::Meeting`]), whose JSON is this struct's own.
///
/// A [`TranscribedMeeting`] reads as one with no copy, which is what the step after receives
/// when this one is skipped, by a person or because nothing was said. Read the copy it names
/// with [`corrected_lines`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectedMeeting {
    pub meeting_id: Uuid,
    /// The capture corrected; `None` in what a run started before meetings had captures was
    /// handed (`super::capture_of`).
    #[serde(default)]
    pub capture_id: Option<Uuid>,
    /// The SHA-256 of the source's stored transcript, as hex; `None` when the meeting was not
    /// fetched.
    #[serde(default)]
    pub transcript_hash: Option<String>,
    pub recording: Option<StoredRecording>,
    /// The SHA-256 of the Whisper transcript stored for the meeting; `None` when it was not
    /// transcribed.
    #[serde(default)]
    pub whisper_transcript_hash: Option<String>,
    /// The SHA-256 of the meeting's corrected copy (`meeting_corrections.transcript_hash`);
    /// `None` when it was not corrected.
    #[serde(default)]
    pub corrected_transcript_hash: Option<String>,
    /// Whether its person had approved the corrected copy when the run went on past it.
    #[serde(default)]
    pub approved: bool,
}

/// Corrects one meeting's transcript. See the module's docs.
#[derive(Clone)]
pub struct CorrectTranscript {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    events: EventBus,
    runner: Runner,
}

impl std::fmt::Debug for CorrectTranscript {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CorrectTranscript")
            .finish_non_exhaustive()
    }
}

/// What [`CorrectTranscript`] is built from.
#[derive(Clone)]
pub struct CorrectTranscriptDeps {
    pub database: Pool,
    pub events: EventBus,
    pub runner: Runner,
}

/// What reading a meeting for its correction found.
enum Read {
    /// Nothing was heard in it, so there is nothing to correct.
    Silent,
    Heard(Box<Heard>),
}

/// A meeting's final transcript, every capture's version of it, and everything its correction
/// reads beside them.
struct Heard {
    meeting: Meeting,
    /// Its first capture's lines, which the corrected copy follows.
    base: Base,
    /// Where each of the first capture's words stands in those lines.
    places: HashMap<Uuid, (usize, usize)>,
    words: Vec<Word>,
    versions: Vec<Version>,
    /// What each capture's source is called, by capture.
    sources: HashMap<Uuid, String>,
    transcription: Vec<transcription_doc::Entry>,
    doc: company_doc::CompanyDocResponse,
    speakers: Vec<String>,
}

/// What one turn's answer settles, checked.
struct Settling {
    flags: Vec<Flag>,
    changed: Vec<Changed>,
}

/// What storing a settlement found.
enum Stored {
    /// This attempt settled the meeting: its `settled` history entry, its copy, whether the
    /// Transcription doc learned from it, and the names it suggested to the company doc.
    Settled {
        entry: Box<MeetingEvent>,
        correction: MeetingCorrection,
        learned: bool,
        suggested: Vec<Suggested>,
    },
    /// Another attempt settled it first; its copy is kept, and this one's discarded.
    ByAnother(MeetingCorrection),
}

/// Whether the run goes on past the correction, as decided under the meeting's lock.
enum Decision {
    GoOn {
        approved: bool,
    },
    /// The run waits for its person; `raised` says whether this attempt raised the inbox item
    /// asking for the review, rather than finding one an earlier attempt raised.
    Hold {
        raised: bool,
    },
    /// Its person deleted the meeting while it was corrected: the copy is kept, nobody is asked
    /// to review it, and the run stops until they restore it, which finds the copy stored.
    Stopped,
}

impl CorrectTranscript {
    pub fn new(deps: CorrectTranscriptDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                database: deps.database,
                events: deps.events,
                runner: deps.runner,
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

    /// The meeting's final transcript, lined up first when its aligning was skipped, every
    /// capture's version of it, and what its correction reads beside them.
    async fn read(&self, input: &TranscribedMeeting, log: &StepLog) -> Result<Read, StepFailure> {
        let mut connection = self.connection().await?;
        let author = Author {
            user_id: SYSTEM_USER_ID,
            agent: None,
            at: Utc::now(),
        };
        let built = final_transcript::rebuild(&mut connection, input.meeting_id, author)
            .await
            .map_err(StepFailure::retryable)?;
        if let Some(built) = built {
            log.line(&format!(
                "Lined the final transcript up: {} words",
                built.data["words"]
            ));
        }
        let read = async {
            let meeting = meeting::find(&mut connection, input.meeting_id).await?;
            let first =
                super::capture_of(&mut connection, input.meeting_id, input.capture_id).await?;
            let (source, whisper) = final_transcript::heard_by(&mut connection, &first).await?;
            let first_whisper = final_transcript::words_of(whisper.as_ref());
            let words = meeting_word::standing(&mut connection, meeting.id).await?;
            let Some(base) = correction::base(source, whisper.as_ref()) else {
                return anyhow::Ok(Read::Silent);
            };
            if words.is_empty() {
                return Ok(Read::Silent);
            }
            let places = correction::places_in_lines(
                &base,
                &first_whisper,
                first.offset_ms,
                first.id,
                &words,
            );
            let (versions, sources) = versions_of(&mut connection, meeting.id).await?;
            let transcription = transcription_doc::read(&mut connection).await?;
            let doc = company_doc::read(&mut connection).await?;
            let speakers = meeting_correction::recent_speakers(
                &mut connection,
                meeting.id,
                OTHER_MEETINGS,
                SPEAKERS_MAX,
            )
            .await?;
            Ok(Read::Heard(Box::new(Heard {
                meeting,
                base,
                places,
                words,
                versions,
                sources,
                transcription: transcription.entries,
                doc,
                speakers,
            })))
        };
        read.await.map_err(StepFailure::retryable)
    }

    /// Asks the correcting agent about `heard` in one turn over the whole meeting, and checks its
    /// answer.
    async fn ask_about(
        &self,
        heard: &Heard,
        context: &StepContext,
    ) -> Result<Settling, StepFailure> {
        let participants: Vec<String> =
            serde_json::from_value(heard.meeting.participants.clone()).unwrap_or_default();
        let background = correction::background(Background {
            title: &heard.meeting.title,
            participants: &participants,
            transcription: &heard.transcription,
            doc: &heard.doc,
            speakers: &heard.speakers,
        });
        let transcript = Transcript::new(&heard.words, heard.versions.clone());
        context.log.line(&format!(
            "Settling {} words in {} lines, beside {} transcripts of its captures, in one turn",
            heard.words.len(),
            transcript.lines.len(),
            heard.versions.len(),
        ));

        let model = context.model().await?;
        let prompt = correction::prompt(&background, &transcript);
        let answer = self.ask(heard.meeting.id, prompt, &model).await?;
        let checked = correction::parse_answer(&answer, &transcript)
            .map_err(|error| StepFailure::retryable(format!("The correction's answer: {error}")))?;
        for (line, refusal) in &checked.refused {
            let named = match line {
                Some(line) => format!("line {line}"),
                None => "a line named by no number".to_owned(),
            };
            context
                .log
                .line(&format!("Refused a flag on {named}: {refusal}"));
        }
        let changed = correction::unflagged_changes(&transcript, &checked.flags);
        context.log.line(&format!(
            "{} flags kept, {} of them changing words, and {} refused; {} runs a capture heard \
             otherwise",
            checked.flags.len(),
            checked
                .flags
                .iter()
                .filter(|flag| flag.changes_text)
                .count(),
            checked.refused.len(),
            changed.len(),
        ));
        Ok(Settling {
            flags: checked.flags,
            changed,
        })
    }

    /// One correction turn on the step's model, answering the model's final text.
    async fn ask(
        &self,
        meeting_id: Uuid,
        prompt: String,
        model: &ModelChoice,
    ) -> Result<String, StepFailure> {
        let request = TurnRequest {
            conversation: ConversationId::new(Uuid::now_v7()),
            label: format!("meeting:{meeting_id}:correction"),
            prompt,
            working_directory: WorkspacePath::new(CORRECTION_DIRECTORY)
                .expect("the correction directory is a plain relative path"),
            resume: None,
            mcp_servers: Vec::new(),
            allowed_tools: None,
            system_prompt_append: None,
            environment: Vec::new(),
            model: Some(model.model.clone()),
            effort: Some(model.effort),
            mode: TurnMode::Text {
                system_prompt: correction::SYSTEM_PROMPT.to_owned(),
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
                        "The correction turn ended without an outcome",
                    ));
                }
            }
        };
        answer_of(outcome)
    }

    /// Settles `heard`'s final transcript with `settling` and stores its corrected copy, under the
    /// meeting's lock in one transaction, teaching the Transcription doc and proposing to the
    /// company doc what it learned. Another attempt's copy stored first is kept instead.
    async fn store(
        &self,
        heard: &Heard,
        settling: &Settling,
        input: &TranscribedMeeting,
        context: &StepContext,
    ) -> Result<Stored, StepFailure> {
        let source_transcript_hash = input
            .transcript_hash
            .clone()
            .ok_or_else(|| StepFailure::permanent("The meeting was not fetched"))?;
        let ranges = settled_ranges(&heard.words, settling);
        let participants: Vec<String> =
            serde_json::from_value(heard.meeting.participants.clone()).unwrap_or_default();
        let mut people = participants;
        people.extend(heard.words.iter().map(|word| word.speaker.clone()));
        people.sort();
        people.dedup();
        let author = Author {
            user_id: SYSTEM_USER_ID,
            agent: None,
            at: Utc::now(),
        };
        // A name the company doc lacks is suggested to it in Elysia's name, naming the run, as
        // its review's suggestions are.
        let as_elysia = Author {
            user_id: ELYSIA_USER_ID,
            agent: None,
            at: Utc::now(),
        };

        let mut connection = self.connection().await?;
        let meeting_id = heard.meeting.id;
        let stored = connection
            .transaction(async |connection| {
                meeting::lock(connection, meeting_id).await?;
                if let Some(stored) = meeting_correction::find(connection, meeting_id).await? {
                    return anyhow::Ok(Stored::ByAnother(stored));
                }
                let settled = meeting_word::settle(connection, meeting_id, &ranges, author).await?;
                // Settling rewrites words a comment thread may quote, as an edit does, so the
                // segments that index the threads over them are indexed again.
                super::comments::reindex_meeting(connection, &heard.meeting).await?;
                // The flags are the first ranges (`settled_ranges`), so the first answers are
                // theirs. A flag whose words changed since the turn read them was passed over, and
                // so is left out of the copy as of the words.
                let applied: Vec<&Flag> = settling
                    .flags
                    .iter()
                    .zip(&settled.marked[..settling.flags.len()])
                    .filter(|(_flag, marked)| marked.is_some())
                    .map(|(flag, _marked)| flag)
                    .collect();
                let (copy, changed_lines) = correction::corrected_copy(
                    &heard.base,
                    &heard.places,
                    &heard.words,
                    applied.iter().copied(),
                );
                let new = NewMeetingCorrection {
                    meeting_id,
                    transcript: serde_json::to_value(&copy)?,
                    transcript_hash: correction::transcript_hash(&copy),
                    based_on: heard.base.based_on.as_str().to_owned(),
                    source_transcript_hash: source_transcript_hash.clone(),
                    whisper_transcript_hash: input.whisper_transcript_hash.clone(),
                    changed_lines: i32::try_from(changed_lines).unwrap_or(i32::MAX),
                    fixes: serde_json::to_value(correction::distinct_fixes(
                        applied.iter().copied(),
                    ))?,
                };
                let correction = meeting_correction::record(connection, new)
                    .await?
                    .ok_or_else(|| anyhow!("the meeting's copy was stored under its own lock"))?;
                let lessons = lessons_of(heard, &applied);
                let learned = transcription_doc::learn(connection, &lessons, author).await?;
                let suggested = transcription_doc::suggest_unknown(
                    connection,
                    &lessons,
                    &people,
                    (as_elysia, Some(context.run_id)),
                )
                .await?;
                context.log.line(&format!(
                    "Settled {} ranges and passed over {} whose words changed since",
                    settled.marked.iter().flatten().count(),
                    settled
                        .marked
                        .iter()
                        .filter(|range| range.is_none())
                        .count(),
                ));
                Ok(Stored::Settled {
                    entry: Box::new(settled.entry),
                    correction,
                    learned,
                    suggested,
                })
            })
            .await
            .map_err(StepFailure::retryable)?;
        Ok(stored)
    }

    /// Whether the run waits for the meeting's person, decided under its lock, raising the
    /// inbox item that asks for the review when it waits and none was raised.
    async fn decide(&self, meeting_id: Uuid) -> Result<Decision, StepFailure> {
        let mut connection = self.connection().await?;
        let (decision, raised) = connection
            .transaction(async move |connection| {
                let current = meeting::lock(connection, meeting_id).await?;
                if current.deleted_at.is_some() {
                    return Ok((Decision::Stopped, None));
                }
                let correction = meeting_correction::find(connection, meeting_id)
                    .await?
                    .ok_or_else(|| anyhow!("the meeting's corrected copy went missing"))?;
                if correction.approved_at.is_some() {
                    return Ok((Decision::GoOn { approved: true }, None));
                }
                if !waits_for_approval(connection, &current).await? {
                    return Ok((Decision::GoOn { approved: false }, None));
                }
                if correction.review_item_id.is_some() {
                    return Ok((Decision::Hold { raised: false }, None));
                }
                let item = review_item(&current);
                let raised = action_item::create(connection, item, Actor::System, Utc::now())
                    .await
                    .map_err(|error| anyhow!("cannot raise the review's inbox item: {error}"))?;
                meeting_correction::set_review_item(connection, meeting_id, raised.record.id)
                    .await?;
                anyhow::Ok((Decision::Hold { raised: true }, Some(raised)))
            })
            .await
            .map_err(StepFailure::retryable)?;

        if let Some(raised) = raised
            && let Err(error) =
                publish_item_write(&self.inner.events, &mut connection, raised, &[], Utc::now())
                    .await
        {
            // The item is stored; clients that missed it read it on their next load.
            tracing::event!(
                name: "meetings.correction.unpublished",
                tracing::Level::WARN,
                meeting.id = %meeting_id,
                error.message = %error,
                "the review's inbox item was raised but could not be announced",
            );
        }
        Ok(decision)
    }

    /// Settles the meeting `input` names in one turn, and answers its stored copy.
    async fn settle(
        &self,
        input: &TranscribedMeeting,
        context: &StepContext,
    ) -> Result<Option<MeetingCorrection>, StepFailure> {
        let Read::Heard(heard) = self.read(input, &context.log).await? else {
            return Ok(None);
        };
        let settling = self.ask_about(&heard, context).await?;
        match self.store(&heard, &settling, input, context).await? {
            Stored::Settled {
                entry,
                correction,
                learned,
                suggested,
            } => {
                context.log.line(&format!(
                    "Stored the corrected copy, SHA-256 {}",
                    correction.transcript_hash
                ));
                if learned {
                    context.log.line("The Transcription doc learned from it");
                    self.inner.events.publish(&ServerEvent::PageChanged {
                        id: transcription_doc::TRANSCRIPTION_PAGE_ID,
                    });
                }
                let mut connection = self.connection().await?;
                if !suggested.is_empty() {
                    context.log.line(&format!(
                        "Suggested {} names to the company doc, for a person to accept",
                        suggested.len()
                    ));
                }
                for each in suggested {
                    pages::announce_suggested(&self.inner.events, &mut connection, each).await;
                }
                // Every page reads the settled transcript again, as after any change no page made.
                if let Err(error) =
                    announce_lined_up(&self.inner.events, &mut connection, &entry).await
                {
                    // The meeting is settled; pages that missed it read it on their next load.
                    tracing::event!(
                        name: "meetings.correction.unannounced",
                        tracing::Level::WARN,
                        meeting.id = %heard.meeting.id,
                        error.message = %error,
                        "a settled meeting could not be announced",
                    );
                }
                Ok(Some(correction))
            }
            Stored::ByAnother(stored) => {
                context.log.line(&format!(
                    "Another attempt settled the meeting first, SHA-256 {}; it is kept",
                    stored.transcript_hash
                ));
                Ok(Some(stored))
            }
        }
    }
}

impl Step for CorrectTranscript {
    const NAME: &'static str = "correct_transcript";
    /// It reads the transcripts and corrects them in an agent turn.
    const AGENT_KIND: Option<AgentKind> = Some(AgentKind::Correction);
    type Input = TranscribedMeeting;
    type Output = TranscriptToSegment;

    async fn run(
        &self,
        input: TranscribedMeeting,
        context: StepContext,
    ) -> Result<Done<TranscriptToSegment>, StepFailure> {
        if input.transcript_hash.is_none() {
            context.log.line("The meeting's fetch was skipped");
            return Ok(Done::Skipped {
                reason: "Not fetched, so nothing to correct".to_owned(),
            });
        }
        let (current, stored) = {
            let mut connection = self.connection().await?;
            let current = meeting::find(&mut connection, input.meeting_id)
                .await
                .map_err(StepFailure::retryable)?;
            let stored = meeting_correction::find(&mut connection, input.meeting_id)
                .await
                .map_err(StepFailure::retryable)?;
            (current, stored)
        };
        // A meeting its person deleted stops its run here until they restore it.
        if current.deleted_at.is_some() {
            return Err(StepFailure::permanent(super::deletion::STOPPED));
        }
        if current.merged_into.is_some() {
            context
                .log
                .line("The meeting's captures were merged into another meeting");
            return Ok(Done::Skipped {
                reason: "Merged into another meeting, so nothing to correct".to_owned(),
            });
        }
        let correction = if let Some(stored) = stored {
            context.log.line(&format!(
                "Already corrected, SHA-256 {}; nothing was changed",
                stored.transcript_hash
            ));
            stored
        } else {
            let Some(settled) = self.settle(&input, &context).await? else {
                context.log.line("Neither transcript holds anything said");
                return Ok(Done::Skipped {
                    reason: "Nothing was said, so nothing to correct".to_owned(),
                });
            };
            settled
        };

        let summary = summary_of(&correction);
        let output = |approved: bool| {
            TranscriptToSegment::Meeting(CorrectedMeeting {
                meeting_id: input.meeting_id,
                capture_id: input.capture_id,
                transcript_hash: input.transcript_hash.clone(),
                recording: input.recording.clone(),
                whisper_transcript_hash: input.whisper_transcript_hash.clone(),
                corrected_transcript_hash: Some(correction.transcript_hash.clone()),
                approved,
            })
        };
        match self.decide(input.meeting_id).await? {
            Decision::GoOn { approved } => {
                context.log.line(&summary);
                Ok(Done::Succeeded {
                    output: output(approved),
                    summary: Some(summary),
                })
            }
            Decision::Hold { raised } => {
                if raised {
                    context
                        .log
                        .line("Raised an inbox item to review the corrected copy");
                }
                context.log.line(&format!(
                    "{summary}; the run waits for its person's approval"
                ));
                Ok(Done::Held {
                    output: output(false),
                    summary: format!("{summary}; waiting for approval"),
                })
            }
            Decision::Stopped => {
                context.log.line(&format!(
                    "{summary}; the meeting was deleted meanwhile, so the run stops here"
                ));
                Err(StepFailure::permanent(super::deletion::STOPPED))
            }
        }
    }
}

#[cfg(test)]
impl CorrectTranscript {
    /// The step over `database`, with a runner whose workspace Docker never answers: enough for
    /// a meeting corrected already, or one with nothing said.
    pub fn for_test(database: Pool) -> Self {
        let docker = bollard::Docker::connect_with_http(
            "http://127.0.0.1:9",
            5,
            bollard::API_DEFAULT_VERSION,
        )
        .expect("a static address parses");
        let runner = Runner::new(
            crate::runner::workspace::WorkspaceContainer::new(
                docker,
                "elysium-workspace:test".to_owned(),
            ),
            database.clone(),
            Arc::new(crate::test_support::cipher()),
            crate::activity::Activities::new(),
            crate::claude_accounts::TurnSignal::default(),
            &tokio_util::sync::CancellationToken::new(),
        );
        Self::new(CorrectTranscriptDeps {
            database,
            events: EventBus::in_process(),
            runner,
        })
    }
}

/// What a finished correction turn answered: the model's final text, or why the step failed. A
/// prompt past the model's context fails for good, since the prompt is the meeting itself and
/// every attempt would read the same; a person who skips the step lets segmenting go on from the
/// transcript as heard. Any other failure is tried again with the run's backoff.
fn answer_of(outcome: TurnOutcome) -> Result<String, StepFailure> {
    if outcome.prompt_too_long() {
        return Err(StepFailure::permanent(format!(
            "The meeting is too long for the model to read in one turn: {}",
            outcome.error.unwrap_or_default()
        )));
    }
    if outcome.status != TurnStatus::Completed {
        return Err(StepFailure::retryable(anyhow!(
            "The correction turn ended {:?}: {}",
            outcome.status,
            outcome.error.unwrap_or_default()
        )));
    }
    Ok(outcome.result.unwrap_or_default())
}

/// Every capture's own transcripts of meeting `meeting_id` on its clock, its source's and its
/// Whisper's, labelled as the prompt names them, and what each capture's source is called.
async fn versions_of(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> anyhow::Result<(Vec<Version>, HashMap<Uuid, String>)> {
    let captures = meeting_capture::list_of(connection, meeting_id).await?;
    let mut versions = Vec::new();
    let mut sources = HashMap::new();
    for (number, capture) in (1_usize..).zip(&captures) {
        let source_name = super::source_name(&capture.source_kind);
        sources.insert(capture.id, source_name.clone());
        let (source, whisper) = final_transcript::heard_by(connection, capture).await?;
        let whisper_words: Vec<WhisperWord> = final_transcript::words_of(whisper.as_ref());
        let label = |name: &str| {
            if captures.len() == 1 {
                name.to_owned()
            } else {
                format!("Capture {number}, {name}")
            }
        };
        // A transcript with no times, as an upload may be, has no moment to set beside a line.
        if !source.is_empty() && !crate::meeting_uploads::transcript::is_untimed(&source) {
            versions.push(Version::of(
                label(&source_name),
                &source,
                &whisper_words,
                capture.offset_ms,
            ));
        }
        if let Some(whisper) = &whisper {
            versions.push(Version::of(
                label("Whisper"),
                &correction::whisper_lines(whisper),
                &whisper_words,
                capture.offset_ms,
            ));
        }
    }
    Ok((versions, sources))
}

/// What settling writes: each flag over its words, in order, with the text it decided on when
/// that differs, then each run marked changed. None overlaps another, so the order they are written
/// in changes nothing but which of [`meeting_word::settle`]'s answers is whose.
fn settled_ranges(words: &[Word], settling: &Settling) -> Vec<SettledRange> {
    let ids = |range: &std::ops::Range<usize>| -> Vec<Uuid> {
        words[range.clone()].iter().map(|word| word.id).collect()
    };
    let flagged = settling.flags.iter().map(|flag| SettledRange {
        words: ids(&flag.words),
        text: flag.changes_text.then(|| flag.text.clone()),
        mark: flag.mark(),
    });
    let changed = settling.changed.iter().map(|changed| SettledRange {
        words: ids(&changed.words),
        text: None,
        mark: changed.mark(),
    });
    flagged.chain(changed).collect()
}

/// What the Transcription doc learns from `applied`, the flags settled over the words they were
/// read from: each fix of each one the agent was sure of, with the transcript and speaker its
/// words were heard from.
fn lessons_of(heard: &Heard, applied: &[&Flag]) -> Vec<Lesson> {
    let meeting = MeetingLink {
        id: heard.meeting.id,
        title: heard.meeting.title.clone(),
    };
    let mut lessons = Vec::new();
    for flag in applied
        .iter()
        .filter(|flag| flag.confidence >= TEACHING_CONFIDENCE)
    {
        let first = &heard.words[flag.words.start];
        let source = match first.origin.as_str() {
            origin if origin == Origin::Whisper.as_str() => Some("Whisper".to_owned()),
            origin if origin == Origin::Source.as_str() => first
                .capture_id
                .and_then(|capture_id| heard.sources.get(&capture_id).cloned()),
            _ => None,
        };
        let speaker = Some(first.speaker.clone()).filter(|speaker| !speaker.is_empty());
        let why = match &flag.why {
            Some(why) => format!(
                "The meeting's correction was {} of 100 sure: {why}",
                flag.confidence
            ),
            None => format!(
                "The meeting's correction was {} of 100 sure.",
                flag.confidence
            ),
        };
        for fix in &flag.fixes {
            lessons.push(Lesson {
                heard: fix.heard.clone(),
                written: fix.written.clone(),
                source: source.clone(),
                speaker: speaker.clone(),
                meeting: meeting.clone(),
                why: why.clone(),
            });
        }
    }
    lessons
}

/// The one line a correction's row collapses to: how many lines it changed, never what.
fn summary_of(correction: &MeetingCorrection) -> String {
    let count = correction.transcript.as_array().map_or(0, Vec::len);
    let lines = if count == 1 {
        "1 line".to_owned()
    } else {
        format!("{count} lines")
    };
    if correction.changed_lines == 0 {
        return format!("Nothing needed correcting in {lines}");
    }
    format!("Corrected {} of {lines}", correction.changed_lines)
}

/// Whether `current` waits for its person's approval before it is acted on: held by hand, or
/// the source of its capture has "Wait for my approval before acting on meetings" on. A meeting
/// whose connector was removed follows its own hold alone.
async fn waits_for_approval(
    connection: &mut AsyncPgConnection,
    current: &Meeting,
) -> diesel::QueryResult<bool> {
    if current.hold_for_approval {
        return Ok(true);
    }
    let capture = meeting_capture::first_of(connection, current.id).await?;
    let Some(connector_id) = capture.connector_id else {
        return Ok(false);
    };
    match connector::find(connection, connector_id).await {
        Ok(source) => Ok(super::waits_for_approval(&source)),
        Err(diesel::result::Error::NotFound) => Ok(false),
        Err(error) => Err(error),
    }
}

/// The inbox item asking `current`'s person to review its corrected transcript. Items are the
/// whole workspace's, so it carries the meeting's title, as an item a person's mail raises
/// carries its subject (`docs/knowledge.md`, Who sees what), and nothing that was said.
fn review_item(current: &Meeting) -> NewActionItem {
    let title = if current.title.trim().is_empty() {
        "Approve a meeting's corrected transcript".to_owned()
    } else {
        format!(
            "Approve the corrected transcript of {}",
            current.title.trim()
        )
    };
    NewActionItem {
        title: title.chars().take(ITEM_TITLE_MAX_CHARACTERS).collect(),
        notes: "The meeting waits for your approval of its corrected transcript before Elysium \
                acts on it. Its page under Settings, Meetings shows what correction changed, \
                with Approve."
            .to_owned(),
        state: ActionItemState::Inbox,
        priority: ActionItemPriority::Normal,
        due_at: None,
        owner: Owner::User,
        project_ids: Vec::new(),
        initiative_ids: Vec::new(),
    }
}

/// The lines of the corrected copy `corrected` names, when it names one that is stored: what a
/// step after the correction reads in place of the raw transcript. `None` when the meeting was
/// not corrected, and the raw transcript is what there is.
///
/// # Errors
/// Propagates any database error, and a stored copy that does not read as transcript lines.
pub async fn corrected_lines(
    connection: &mut AsyncPgConnection,
    corrected: &CorrectedMeeting,
) -> anyhow::Result<Option<Vec<TranscriptLine>>> {
    let Some(hash) = &corrected.corrected_transcript_hash else {
        return Ok(None);
    };
    let Some(stored) = meeting_correction::find(connection, corrected.meeting_id).await? else {
        return Ok(None);
    };
    if &stored.transcript_hash != hash {
        return Ok(None);
    }
    let lines = serde_json::from_value(stored.transcript)
        .map_err(|error| anyhow!("the corrected copy is not in the stored shape: {error}"))?;
    Ok(Some(lines))
}

/// Why a correction could not be approved.
#[derive(Debug, thiserror::Error)]
pub enum ApproveError {
    /// The meeting is not its person's, does not exist, or has no corrected copy yet.
    #[error("not found")]
    NotFound,
    /// The meeting was merged into another, which is where it changes now.
    #[error("the meeting was merged into another")]
    Merged,
    /// The meeting is deleted, so it changes no further until it is restored.
    #[error("the meeting is deleted")]
    Deleted,
    #[error(transparent)]
    Run(#[from] StepActionError),
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// Approves meeting `meeting_id`'s corrected copy as `owner`, its person: records who approved
/// it, resolves the inbox item that asked for it, and resumes its run when the correction holds
/// it, the step's output saying the copy was approved. All in one transaction; then announced.
/// Answers the meeting.
///
/// # Errors
/// [`ApproveError::NotFound`] when the meeting is not `owner`'s or has no corrected copy,
/// [`ApproveError::Merged`] once it was merged into another, [`ApproveError::Deleted`] while it
/// is deleted; any database error.
pub async fn approve(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    meeting_id: Uuid,
    owner: Uuid,
) -> Result<Meeting, ApproveError> {
    let now = Utc::now();
    let (current, resumed, resolved) = connection
        .transaction(async move |connection| {
            let current = meeting::lock(connection, meeting_id).await?;
            if current.created_by != owner {
                return Err(ApproveError::NotFound);
            }
            // Merged into another, its run goes no further and its words change there.
            if current.merged_into.is_some() {
                return Err(ApproveError::Merged);
            }
            // Deleted, its run is stopped until it is restored.
            if current.deleted_at.is_some() {
                return Err(ApproveError::Deleted);
            }
            let already = meeting_correction::find(connection, meeting_id)
                .await?
                .ok_or(ApproveError::NotFound)?
                .approved_at
                .is_some();
            let correction = meeting_correction::approve(connection, meeting_id, owner, now)
                .await
                .map_err(|error| match error {
                    diesel::result::Error::NotFound => ApproveError::NotFound,
                    other => ApproveError::Database(other),
                })?;
            if !already {
                let author = Author {
                    user_id: owner,
                    agent: None,
                    at: now,
                };
                meeting_event::record(
                    connection,
                    Change {
                        id: Uuid::now_v7(),
                        meeting_id,
                        kind: EventKind::CorrectionApproved,
                        author,
                        data: serde_json::json!({}),
                        undid: None,
                    },
                )
                .await?;
            }
            let resolved = match correction.review_item_id {
                Some(item_id) => resolve_review_item(connection, item_id, owner, now).await?,
                None => None,
            };
            let resumed = resume_held_correction(connection, meeting_id, owner, now).await?;
            Ok::<_, ApproveError>((current, resumed, resolved))
        })
        .await?;

    if let Some(settled) = &resumed {
        workflows::announce(events, settled);
        events.signal(&Signal::Workflows);
    }
    if let Some(resolved) = resolved
        && let Err(error) = publish_item_write(events, connection, resolved, &[], now).await
    {
        // The item is resolved; clients that missed it read it on their next load.
        tracing::event!(
            name: "meetings.approval.unpublished",
            tracing::Level::WARN,
            meeting.id = %meeting_id,
            error.message = %error,
            "a review item was resolved but could not be announced",
        );
    }
    events.publish(&ServerEvent::MeetingChanged {
        id: current.id,
        state: current.state.clone(),
        created_by: current.created_by,
        entry_id: None,
    });
    Ok(current)
}

/// Why a meeting's hold could not change.
#[derive(Debug, thiserror::Error)]
pub enum HoldError {
    /// The meeting is not its person's, or does not exist.
    #[error("not found")]
    NotFound,
    /// Its corrected copy is stored: approving it, or not, is what remains.
    #[error("the meeting's transcript is corrected already")]
    Corrected,
    /// The meeting was merged into another, which is where it changes now.
    #[error("the meeting was merged into another")]
    Merged,
    /// The meeting is deleted, so it changes no further until it is restored.
    #[error("the meeting is deleted")]
    Deleted,
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// Sets whether meeting `meeting_id` waits for `owner`'s approval of its corrected transcript
/// before it is acted on, whatever its source's setting, records the change in its history, and
/// announces it. The correction step reads it as it decides, under the same lock, so a hold set
/// before its copy is stored is always honoured; once a copy is stored, the hold no longer
/// changes.
///
/// # Errors
/// [`HoldError::NotFound`] when the meeting is not `owner`'s, [`HoldError::Corrected`] once
/// its copy is stored, [`HoldError::Merged`] once it was merged into another,
/// [`HoldError::Deleted`] while it is deleted; any database error.
pub async fn hold(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    meeting_id: Uuid,
    owner: Uuid,
    hold: bool,
) -> Result<Meeting, HoldError> {
    let changed = connection
        .transaction(async move |connection| {
            let current = meeting::lock(connection, meeting_id).await?;
            if current.created_by != owner {
                return Err(HoldError::NotFound);
            }
            if current.merged_into.is_some() {
                return Err(HoldError::Merged);
            }
            if current.deleted_at.is_some() {
                return Err(HoldError::Deleted);
            }
            if meeting_correction::find(connection, meeting_id)
                .await?
                .is_some()
            {
                return Err(HoldError::Corrected);
            }
            if current.hold_for_approval == hold {
                return Ok(current);
            }
            let changed = meeting::set_hold_for_approval(connection, meeting_id, hold).await?;
            let author = Author {
                user_id: owner,
                agent: None,
                at: Utc::now(),
            };
            super::history::record_hold(connection, meeting_id, (!hold, hold), author, None)
                .await?;
            Ok(changed)
        })
        .await?;
    events.publish(&ServerEvent::MeetingChanged {
        id: changed.id,
        state: changed.state.clone(),
        created_by: changed.created_by,
        entry_id: None,
    });
    Ok(changed)
}

/// Resolves review item `item_id` as `owner`, unless it is resolved, dismissed, or deleted
/// already: the person may have dealt with it from Action items first.
async fn resolve_review_item(
    connection: &mut AsyncPgConnection,
    item_id: Uuid,
    owner: Uuid,
    now: chrono::DateTime<Utc>,
) -> diesel::QueryResult<Option<Recorded<ActionItem>>> {
    let resolving = Actor::User(owner);
    match action_item::transition(connection, item_id, Transition::Resolve, resolving, now).await {
        Ok(resolved) => Ok(Some(resolved)),
        Err(WorkError::Database(diesel::result::Error::NotFound)) => Ok(None),
        Err(WorkError::Database(error)) => Err(error),
        // Already resolved or dismissed, or deleted: there is nothing left to resolve.
        Err(_refused) => Ok(None),
    }
}

/// Resumes meeting `meeting_id`'s run when its correction step holds it, the held output
/// marked approved, as `owner`.
async fn resume_held_correction(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    owner: Uuid,
    now: chrono::DateTime<Utc>,
) -> Result<Option<Settled>, ApproveError> {
    let source_id = meeting_id.to_string();
    let live = workflow_run::find_live(
        connection,
        super::workflow::workflow().name(),
        super::workflow::SOURCE_KIND,
        &source_id,
    )
    .await?;
    let Some(run) = live else {
        return Ok(None);
    };
    let steps = workflow_run::steps_of(connection, run.id).await?;
    let held = steps
        .into_iter()
        .find(|step| step.step == CorrectTranscript::NAME && step.state == StepJobState::Held);
    let Some(held) = held else {
        return Ok(None);
    };
    let held_output = held
        .output
        .clone()
        .and_then(|output| serde_json::from_value::<TranscriptToSegment>(output).ok());
    let Some(TranscriptToSegment::Meeting(mut output)) = held_output else {
        return Err(ApproveError::Run(StepActionError::Refused(
            "The held correction's output is not in its shape.",
        )));
    };
    output.approved = true;
    let output: Value = serde_json::to_value(TranscriptToSegment::Meeting(output))
        .expect("the output is plain fields");
    let actor = workflow_run::Actor {
        user_id: owner,
        at: now,
    };
    Ok(Some(
        step_job::resume_with(connection, run.id, held.id, actor, output).await?,
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use diesel::{ExpressionMethods, QueryDsl, SelectableHelper};
    use diesel_async::RunQueryDsl;

    use super::*;
    use crate::meetings::fetch::RecordedMeeting;
    use crate::models::meeting::tests::fetched_with;
    use crate::models::meeting_capture::MeetingCapture;
    use crate::models::page::TRANSCRIPTION_PAGE_ID as TRANSCRIPTION_PAGE;
    use crate::models::step_job::StepOutcome;
    use crate::models::workflow_run::WorkflowRunState;
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    fn heard_line(text: &str) -> TranscriptLine {
        TranscriptLine {
            start_ms: 61_000,
            end_ms: 64_000,
            speaker: "Ada".to_owned(),
            text: text.to_owned(),
        }
    }

    /// A meeting titled `title` whose capture heard one line, "Jon is here".
    async fn heard_meeting(
        connection: &mut AsyncPgConnection,
        title: &str,
    ) -> (Meeting, MeetingCapture) {
        let (stored, capture) = fetched_with(connection, json!([heard_line("Jon is here")])).await;
        let titled = diesel::update(crate::database::schema::meetings::table.find(stored.id))
            .set(crate::database::schema::meetings::title.eq(title))
            .returning(Meeting::as_returning())
            .get_result(connection)
            .await
            .expect("titled");
        (titled, capture)
    }

    #[test]
    fn what_the_transcription_gave_reads_as_a_meeting_with_no_corrected_copy() {
        // A skipped correction hands the next step what the transcription gave.
        let transcribed = TranscribedMeeting {
            meeting_id: Uuid::now_v7(),
            capture_id: Some(Uuid::now_v7()),
            transcript_hash: Some("a".repeat(64)),
            recording: None,
            whisper_transcript_hash: Some("b".repeat(64)),
        };
        let read: TranscriptToSegment =
            serde_json::from_value(serde_json::to_value(&transcribed).expect("serializes"))
                .expect("reads");
        let TranscriptToSegment::Meeting(read) = read else {
            panic!("a transcribed meeting reads as a meeting to segment, not {read:?}");
        };
        assert_eq!(
            read.whisper_transcript_hash,
            transcribed.whisper_transcript_hash
        );
        assert_eq!(read.corrected_transcript_hash, None);
        assert_eq!(read.capture_id, transcribed.capture_id);
        assert!(!read.approved);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    #[expect(
        clippy::too_many_lines,
        reason = "one run played from its fetch through the hold to its approval"
    )]
    async fn a_held_correction_raises_one_review_and_approving_it_lets_the_run_go_on() {
        let (url, mut connection) = migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 4)
            .await
            .expect("a pool");
        let events = EventBus::in_process();
        let (stored, capture) = heard_meeting(&mut connection, "Weekly sync").await;
        // Its person holds it by hand; it has no source to say otherwise.
        hold(&mut connection, &events, stored.id, TEST_PERSON_ID, true)
            .await
            .expect("held");
        // Holding it again changes nothing, and so records nothing.
        hold(&mut connection, &events, stored.id, TEST_PERSON_ID, true)
            .await
            .expect("still held");

        // Its run, through the fetch, the transcription, and aligning.
        workflows::start(
            &mut connection,
            &events,
            super::super::workflow::run_for(stored.id),
        )
        .await
        .expect("the run starts");
        let fetched = RecordedMeeting {
            meeting_id: stored.id,
            capture_id: Some(capture.id),
            transcript_hash: Some("a".repeat(64)),
            recording: None,
        };
        let through = [
            serde_json::to_value(&fetched).expect("serializes"),
            serde_json::to_value(&fetched).expect("serializes"),
            serde_json::to_value(&fetched).expect("serializes"),
        ];
        for output in through {
            let at = Utc::now();
            let claims = step_job::claim(&mut connection, at, 1)
                .await
                .expect("claims")
                .claims;
            step_job::finish(
                &mut connection,
                &claims[0],
                StepOutcome::Succeeded {
                    output,
                    summary: None,
                },
                at,
            )
            .await
            .expect("records")
            .expect("the claim holds");
        }

        // A first attempt stored its copy before its engine stopped, so this one asks the
        // model nothing: the runner's workspace never answers.
        let corrected = vec![heard_line("John is here")];
        meeting_correction::record(
            &mut connection,
            NewMeetingCorrection {
                meeting_id: stored.id,
                transcript: serde_json::to_value(&corrected).expect("serializes"),
                transcript_hash: correction::transcript_hash(&corrected),
                based_on: "source".to_owned(),
                source_transcript_hash: "a".repeat(64),
                whisper_transcript_hash: None,
                changed_lines: 1,
                fixes: json!([{ "heard": "Jon", "written": "John" }]),
            },
        )
        .await
        .expect("records")
        .expect("new");
        let step = CorrectTranscript::for_test(database.clone());
        let input = TranscribedMeeting {
            meeting_id: stored.id,
            capture_id: Some(capture.id),
            transcript_hash: Some("a".repeat(64)),
            recording: None,
            whisper_transcript_hash: None,
        };
        let context = StepContext::for_test::<CorrectTranscript>(database.clone());
        let done = step.run(input.clone(), context).await.expect("decides");
        let Done::Held {
            output: TranscriptToSegment::Meeting(output),
            summary,
        } = done
        else {
            panic!("a held meeting holds its run, not {done:?}");
        };
        assert_eq!(summary, "Corrected 1 of 1 line; waiting for approval");
        assert!(!output.approved);
        assert_eq!(
            output.corrected_transcript_hash,
            Some(correction::transcript_hash(&corrected))
        );

        // Another attempt holds again, and raises no second item.
        let first_item = meeting_correction::find(&mut connection, stored.id)
            .await
            .expect("reads")
            .expect("stored")
            .review_item_id
            .expect("an item asks for the review");
        let again = StepContext::for_test::<CorrectTranscript>(database.clone());
        let held_again = step.run(input, again).await.expect("decides");
        assert!(matches!(held_again, Done::Held { .. }));
        let second_item = meeting_correction::find(&mut connection, stored.id)
            .await
            .expect("reads")
            .expect("stored")
            .review_item_id;
        assert_eq!(second_item, Some(first_item));
        let item = action_item::find(&mut connection, first_item)
            .await
            .expect("the item");
        assert_eq!(item.state, ActionItemState::Inbox);
        assert_eq!(
            item.title,
            "Approve the corrected transcript of Weekly sync"
        );
        assert!(
            !item.notes.contains("John"),
            "an item says nothing that was said"
        );

        let at = Utc::now();
        let claims = step_job::claim(&mut connection, at, 1)
            .await
            .expect("claims")
            .claims;
        assert_eq!(claims[0].job.step, CorrectTranscript::NAME);
        let settled = step_job::finish(
            &mut connection,
            &claims[0],
            StepOutcome::Held {
                output: serde_json::to_value(&output).expect("serializes"),
                summary,
            },
            at,
        )
        .await
        .expect("records")
        .expect("the claim holds");
        assert_eq!(settled.run.state, WorkflowRunState::Held);

        // Once corrected, holding no longer changes; approving is what lets it go on.
        assert!(matches!(
            hold(&mut connection, &events, stored.id, TEST_PERSON_ID, false).await,
            Err(HoldError::Corrected)
        ));
        assert!(matches!(
            approve(&mut connection, &events, stored.id, Uuid::now_v7()).await,
            Err(ApproveError::NotFound)
        ));
        approve(&mut connection, &events, stored.id, TEST_PERSON_ID)
            .await
            .expect("approved");

        let run = workflow_run::find(&mut connection, settled.run.id)
            .await
            .expect("the run");
        assert_eq!(run.state, WorkflowRunState::Running, "the run goes on");
        let steps = workflow_run::steps_of(&mut connection, run.id)
            .await
            .expect("steps");
        let correction_step = steps
            .iter()
            .find(|step| step.step == CorrectTranscript::NAME)
            .expect("the correction");
        assert_eq!(correction_step.state, StepJobState::Succeeded);
        let segmenting = steps.last().expect("segmenting");
        assert_eq!(
            (segmenting.step.as_str(), segmenting.state),
            ("segment_meeting", StepJobState::Queued)
        );
        assert_eq!(
            segmenting.input, correction_step.output,
            "segmenting takes what the correction handed on"
        );
        let handed_on: TranscriptToSegment =
            serde_json::from_value(correction_step.output.clone().expect("an output"))
                .expect("reads");
        let TranscriptToSegment::Meeting(handed_on) = handed_on else {
            panic!("a correction hands on a meeting, not {handed_on:?}");
        };
        assert!(handed_on.approved, "the run goes on told it was approved");
        let item = action_item::find(&mut connection, first_item)
            .await
            .expect("the item");
        assert_eq!(item.state, ActionItemState::Resolved);
        let approved = meeting_correction::find(&mut connection, stored.id)
            .await
            .expect("reads")
            .expect("stored");
        assert_eq!(approved.approved_by, Some(TEST_PERSON_ID));
        // Approving again changes nothing.
        approve(&mut connection, &events, stored.id, TEST_PERSON_ID)
            .await
            .expect("approved already");

        // Its history: held once and approved once, however many attempts ran. The stored copy
        // is the whole of its settlement, so an attempt that finds it settles nothing again.
        let history = meeting_event::history(&mut connection, stored.id)
            .await
            .expect("history");
        assert_eq!(
            history
                .iter()
                .map(|entry| (entry.kind.as_str(), entry.created_by))
                .collect::<Vec<_>>(),
            vec![
                ("hold_changed", TEST_PERSON_ID),
                ("correction_approved", TEST_PERSON_ID),
            ]
        );
        assert_eq!(history[0].data, json!({ "before": false, "after": true }));

        // And segmenting reads the copy the run handed on.
        let copy = corrected_lines(&mut connection, &handed_on)
            .await
            .expect("reads");
        assert_eq!(copy, Some(corrected));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_correction_nobody_waits_for_goes_on_unapproved_and_nothing_said_skips() {
        let (url, mut connection) = migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 4)
            .await
            .expect("a pool");
        let (stored, _capture) = heard_meeting(&mut connection, "Standup").await;
        let corrected = vec![heard_line("Jon is here")];
        meeting_correction::record(
            &mut connection,
            NewMeetingCorrection {
                meeting_id: stored.id,
                transcript: serde_json::to_value(&corrected).expect("serializes"),
                transcript_hash: correction::transcript_hash(&corrected),
                based_on: "source".to_owned(),
                source_transcript_hash: "a".repeat(64),
                whisper_transcript_hash: None,
                changed_lines: 0,
                fixes: json!([]),
            },
        )
        .await
        .expect("records")
        .expect("new");
        let step = CorrectTranscript::for_test(database.clone());
        // What a run started before captures was handed names the meeting alone.
        let input = TranscribedMeeting {
            meeting_id: stored.id,
            capture_id: None,
            transcript_hash: Some("a".repeat(64)),
            recording: None,
            whisper_transcript_hash: None,
        };
        let done = step
            .run(
                input,
                StepContext::for_test::<CorrectTranscript>(database.clone()),
            )
            .await
            .expect("decides");
        let Done::Succeeded {
            output: TranscriptToSegment::Meeting(output),
            summary,
        } = done
        else {
            panic!("nobody waits, so the run goes on, not {done:?}");
        };
        assert!(!output.approved);
        assert_eq!(
            summary.as_deref(),
            Some("Nothing needed correcting in 1 line")
        );
        let (silent, silent_capture) = fetched_with(&mut connection, json!([])).await;
        let nothing = TranscribedMeeting {
            meeting_id: silent.id,
            capture_id: Some(silent_capture.id),
            transcript_hash: Some("a".repeat(64)),
            recording: None,
            whisper_transcript_hash: None,
        };
        let skipped = step
            .run(
                nothing,
                StepContext::for_test::<CorrectTranscript>(database.clone()),
            )
            .await
            .expect("skips");
        assert_eq!(
            skipped,
            Done::Skipped {
                reason: "Nothing was said, so nothing to correct".to_owned(),
            }
        );

        // A meeting a person skipped the fetch of reaches correction never fetched, and skips.
        let unfetched = TranscribedMeeting {
            meeting_id: silent.id,
            capture_id: None,
            transcript_hash: None,
            recording: None,
            whisper_transcript_hash: None,
        };
        let not_fetched = step
            .run(
                unfetched,
                StepContext::for_test::<CorrectTranscript>(database),
            )
            .await
            .expect("skips");
        assert_eq!(
            not_fetched,
            Done::Skipped {
                reason: "Not fetched, so nothing to correct".to_owned(),
            }
        );
    }

    #[test]
    fn a_prompt_too_long_for_the_model_fails_the_step_for_good() {
        let too_long = TurnOutcome::without_result(TurnStatus::Failed, "Prompt is too long");
        let failure = answer_of(too_long).expect_err("fails");
        assert!(
            !failure.retryable,
            "every attempt would read the same prompt"
        );
        assert!(
            failure.reason.contains("too long for the model"),
            "{}",
            failure.reason
        );

        let broke = TurnOutcome::without_result(TurnStatus::Failed, "The workspace stopped");
        assert!(answer_of(broke).expect_err("fails").retryable);

        let answered = TurnOutcome {
            result: Some("{\"flags\": []}".to_owned()),
            ..TurnOutcome::without_result(TurnStatus::Completed, "")
        };
        assert_eq!(answer_of(answered).expect("answers"), "{\"flags\": []}");
    }

    /// A Fireflies meeting with Ada and Sharath in it, two lines heard by its source, and what
    /// Whisper heard of them, transcribed: the name and the launch misheard, each its own way.
    async fn misheard_meeting(connection: &mut AsyncPgConnection) -> (Meeting, MeetingCapture) {
        let lines = vec![
            TranscriptLine {
                start_ms: 0,
                end_ms: 6_000,
                speaker: "Ada Lovelace".to_owned(),
                text: "We asked Sheriff about the launch".to_owned(),
            },
            TranscriptLine {
                start_ms: 6_000,
                end_ms: 9_000,
                speaker: "Grace Hopper".to_owned(),
                text: "Ely see um ships Friday".to_owned(),
            },
        ];
        let (stored, capture) = fetched_with(
            connection,
            serde_json::to_value(&lines).expect("serializes"),
        )
        .await;
        let titled = diesel::update(crate::database::schema::meetings::table.find(stored.id))
            .set((
                crate::database::schema::meetings::title.eq("Launch review"),
                crate::database::schema::meetings::participants
                    .eq(json!(["ada@example.com", "sharath.kumar@example.com"])),
            ))
            .returning(Meeting::as_returning())
            .get_result(connection)
            .await
            .expect("titled");
        let heard = crate::whisper::WhisperTranscript {
            model: "Whisper large-v3".to_owned(),
            language: "en".to_owned(),
            language_probability: 0.99,
            duration_ms: 9_000,
            segments: vec![
                crate::whisper::WhisperSegment {
                    start_ms: 0,
                    end_ms: 6_000,
                    text: "We asked Sharath about the lunch".to_owned(),
                    words: Vec::new(),
                },
                crate::whisper::WhisperSegment {
                    start_ms: 6_000,
                    end_ms: 9_000,
                    text: "Elysium ships Friday".to_owned(),
                    words: Vec::new(),
                },
            ],
        };
        crate::models::meeting_transcription::record(
            connection,
            crate::models::meeting_transcription::NewMeetingTranscription {
                capture_id: capture.id,
                transcript: serde_json::to_value(&heard).expect("serializes"),
                transcript_hash: "b".repeat(64),
            },
        )
        .await
        .expect("records")
        .expect("new");
        (titled, capture)
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    #[expect(
        clippy::too_many_lines,
        reason = "one answer followed through to the words, the marks, the copy, and both docs"
    )]
    async fn a_correction_settles_the_words_it_flags_and_marks_what_a_capture_heard_otherwise() {
        let (url, mut connection) = migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 4)
            .await
            .expect("a pool");
        let (stored, capture) = misheard_meeting(&mut connection).await;
        let answer = r#"{"flags": [
            {"line": 0, "heard": "Sheriff", "text": "Sharath", "confidence": 92,
             "alternatives": ["Sharad"], "why": "The participants name a Sharath."},
            {"line": 1, "heard": "Ely see um", "text": "Elysium", "confidence": 60},
            {"line": 1, "heard": "ships Friday", "text": "ships Friday at noon", "confidence": 90},
            {"line": 1, "heard": "Friday", "text": "Friday", "confidence": 0}
        ]}"#;
        let step = CorrectTranscript::new(CorrectTranscriptDeps {
            database: database.clone(),
            events: EventBus::in_process(),
            runner: Runner::answering(&[answer]),
        });
        let run = workflows::start(
            &mut connection,
            &EventBus::in_process(),
            super::super::workflow::run_for(stored.id),
        )
        .await
        .expect("a run");
        let mut context = StepContext::for_test::<CorrectTranscript>(database.clone());
        context.run_id = run.settled.run.id;
        let input = TranscribedMeeting {
            meeting_id: stored.id,
            capture_id: Some(capture.id),
            transcript_hash: Some("a".repeat(64)),
            recording: None,
            whisper_transcript_hash: Some("b".repeat(64)),
        };

        let done = step.run(input.clone(), context).await.expect("settles");

        let Done::Succeeded {
            output: TranscriptToSegment::Meeting(output),
            summary,
        } = done
        else {
            panic!("nobody waits for this meeting, so its run goes on, not {done:?}");
        };
        assert_eq!(summary.as_deref(), Some("Corrected 2 of 2 lines"));

        // The words: what the flags decided, as the correction's own, and every other word as the
        // source heard it, whatever Whisper heard.
        let words = meeting_word::standing(&mut connection, stored.id)
            .await
            .expect("reads");
        assert_eq!(
            meeting_word::text_of(&words),
            "We asked Sharath about the launch Elysium ships Friday"
        );
        let origin_of = |text: &str| -> (String, Option<Uuid>) {
            let word = words.iter().find(|word| word.text == text).expect("a word");
            (word.origin.clone(), word.capture_id)
        };
        assert_eq!(
            origin_of("Sharath"),
            ("correction".to_owned(), Some(capture.id))
        );
        assert_eq!(origin_of("Elysium").0, "correction");
        assert_eq!(origin_of("launch").0, "source");

        // The marks: each flag's confidence and alternatives over its words, and the launch
        // Whisper heard as a lunch at the middle of the scale, with what it heard.
        let marks = crate::models::meeting_mark::standing(&mut connection, stored.id)
            .await
            .expect("marks");
        let marked: Vec<(String, Value)> = marks
            .iter()
            .map(|mark| {
                let start = words
                    .iter()
                    .position(|word| word.id == mark.start_word_id)
                    .expect("on a standing word");
                let end = words
                    .iter()
                    .position(|word| word.id == mark.end_word_id)
                    .expect("on a standing word");
                (
                    meeting_word::text_of(&words[start..=end]),
                    mark.data.clone(),
                )
            })
            .collect();
        assert_eq!(
            marked,
            vec![
                (
                    "Sharath".to_owned(),
                    json!({ "confidence": 92, "alternatives": ["Sharad"] })
                ),
                (
                    "Elysium".to_owned(),
                    json!({ "confidence": 60, "alternatives": [] })
                ),
                (
                    "Friday".to_owned(),
                    json!({ "confidence": 0, "alternatives": [] })
                ),
                (
                    "launch".to_owned(),
                    json!({ "confidence": 50, "alternatives": ["lunch"] })
                ),
            ]
        );
        assert!(marks.iter().all(|mark| mark.kind == "confidence"));
        let history = meeting_event::history(&mut connection, stored.id)
            .await
            .expect("history");
        let kinds: Vec<&str> = history.iter().map(|entry| entry.kind.as_str()).collect();
        assert_eq!(kinds, vec!["built", "settled"]);
        assert_eq!(history[1].data["changes"].as_array().map(Vec::len), Some(2));
        assert_eq!(history[1].data["skipped"], 0);

        // The copy segmenting reads: the source's lines, the settled text in place.
        let copy = corrected_lines(&mut connection, &output)
            .await
            .expect("reads")
            .expect("a copy");
        assert_eq!(
            copy.iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["We asked Sharath about the launch", "Elysium ships Friday"]
        );

        // The Transcription doc learned what the agent was sure of, as Elysium, and not the rest.
        let doc = transcription_doc::read(&mut connection)
            .await
            .expect("the doc");
        assert_eq!(doc.entries.len(), 1, "{}", doc.body);
        let entry = &doc.entries[0];
        assert_eq!(
            (entry.heard.as_str(), entry.written.as_str()),
            ("Sheriff", "Sharath")
        );
        assert_eq!(entry.sources, vec!["Fireflies"]);
        assert_eq!(entry.speakers, vec!["Ada Lovelace"]);
        assert_eq!(entry.meetings[0].id, stored.id);
        assert!(
            doc.body
                .contains("The meeting's correction was 92 of 100 sure: The participants")
        );
        let versions = crate::models::page::versions(&mut connection, TRANSCRIPTION_PAGE)
            .await
            .expect("versions");
        let newest = &versions[0];
        assert_eq!(
            (newest.version, newest.created_by),
            (2, SYSTEM_USER_ID),
            "a version of the page by Elysium"
        );

        // Sharath is a participant the company doc does not name, so People gets a suggestion, in
        // Elysia's name and naming the run, waiting for a person on the section's page.
        let pending = crate::models::company_doc::pending(&mut connection)
            .await
            .expect("suggestions");
        assert_eq!(pending.len(), 1);
        assert_eq!(
            pending[0].section,
            crate::models::company_doc::Section::People
        );
        let suggested = &pending[0].suggestion;
        assert!(
            suggested
                .body
                .contains("- Sharath: often heard as \"Sheriff\"")
        );
        assert_eq!(suggested.workflow_run_id, Some(run.settled.run.id));
        assert_eq!(suggested.created_by, ELYSIA_USER_ID);

        // A capture nudged by hand lines the transcript up again; what was settled stands, marks
        // and all, and nothing is settled again.
        let sharath = words
            .iter()
            .find(|word| word.text == "Sharath")
            .expect("Sharath");
        meeting_capture::set_offset(
            &mut connection,
            capture.id,
            40,
            crate::models::meeting_capture::OffsetSource::Hand,
        )
        .await
        .expect("nudged");
        final_transcript::rebuild(
            &mut connection,
            stored.id,
            Author {
                user_id: TEST_PERSON_ID,
                agent: None,
                at: Utc::now(),
            },
        )
        .await
        .expect("rebuilds")
        .expect("the words moved");
        let after = meeting_word::standing(&mut connection, stored.id)
            .await
            .expect("reads");
        assert_eq!(
            meeting_word::text_of(&after),
            "We asked Sharath about the launch Elysium ships Friday"
        );
        assert!(
            after.iter().any(|word| word.id == sharath.id),
            "the settled word keeps its id"
        );
        let marks_after = crate::models::meeting_mark::standing(&mut connection, stored.id)
            .await
            .expect("marks");
        let on_sharath = marks_after
            .iter()
            .find(|mark| mark.data["confidence"] == 92)
            .expect("its mark");
        assert_eq!(
            (on_sharath.start_word_id, on_sharath.end_word_id),
            (sharath.id, sharath.id)
        );

        // Another attempt finds the meeting settled and asks nothing: the runner has no answer
        // left to give.
        let again = step
            .run(
                input,
                StepContext::for_test::<CorrectTranscript>(database.clone()),
            )
            .await
            .expect("answers what was stored");
        assert!(matches!(again, Done::Succeeded { .. }));
        let history = meeting_event::history(&mut connection, stored.id)
            .await
            .expect("history");
        assert_eq!(
            history
                .iter()
                .filter(|entry| entry.kind == "settled")
                .count(),
            1
        );
    }

    /// #269 review: a person who fixes a word while the correction's turn runs keeps their fix, and
    /// the corrected copy leaves out what the correction would have written over it.
    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_flag_over_words_a_person_edited_meanwhile_is_left_out_of_the_words_and_the_copy() {
        let (url, mut connection) = migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 4)
            .await
            .expect("a pool");
        let (stored, capture) = misheard_meeting(&mut connection).await;
        let answer = r#"{"flags": [
            {"line": 0, "heard": "Sheriff", "text": "Sharath", "confidence": 92},
            {"line": 1, "heard": "Ely see um", "text": "Elysium", "confidence": 95}
        ]}"#;
        let step = CorrectTranscript::new(CorrectTranscriptDeps {
            database: database.clone(),
            events: EventBus::in_process(),
            runner: Runner::answering(&[answer]),
        });
        let input = TranscribedMeeting {
            meeting_id: stored.id,
            capture_id: Some(capture.id),
            transcript_hash: Some("a".repeat(64)),
            recording: None,
            whisper_transcript_hash: Some("b".repeat(64)),
        };
        // The run its suggestions to the company doc name.
        let run = workflows::start(
            &mut connection,
            &EventBus::in_process(),
            super::super::workflow::run_for(stored.id),
        )
        .await
        .expect("a run");
        let mut context = StepContext::for_test::<CorrectTranscript>(database.clone());
        context.run_id = run.settled.run.id;

        // The turn reads the meeting, and Ada fixes the name herself before it answers.
        let Read::Heard(heard) = step.read(&input, &context.log).await.expect("reads") else {
            panic!("the meeting was heard");
        };
        let sheriff = heard
            .words
            .iter()
            .find(|word| word.text == "Sheriff")
            .expect("the misheard name")
            .clone();
        meeting_word::edit(
            &mut connection,
            stored.id,
            meeting_word::Replacement {
                range: crate::models::meeting_mark::Range {
                    start_word_id: sheriff.id,
                    end_word_id: sheriff.id,
                },
                text: "Sharad",
            },
            Author {
                user_id: TEST_PERSON_ID,
                agent: None,
                at: Utc::now(),
            },
        )
        .await
        .expect("edits");
        let settling = step.ask_about(&heard, &context).await.expect("asks");
        let Stored::Settled { correction, .. } = step
            .store(&heard, &settling, &input, &context)
            .await
            .expect("stores")
        else {
            panic!("this attempt settles the meeting");
        };

        let words = meeting_word::standing(&mut connection, stored.id)
            .await
            .expect("reads");
        assert_eq!(
            meeting_word::text_of(&words),
            "We asked Sharad about the launch Elysium ships Friday"
        );
        let copy: Vec<TranscriptLine> =
            serde_json::from_value(correction.transcript).expect("lines");
        assert_eq!(
            copy.iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["We asked Sheriff about the launch", "Elysium ships Friday"],
            "the flag passed over is no part of the copy"
        );
        assert_eq!(correction.changed_lines, 1);
        let history = meeting_event::history(&mut connection, stored.id)
            .await
            .expect("history");
        let settled = history
            .iter()
            .find(|entry| entry.kind == "settled")
            .expect("settled");
        assert_eq!(settled.data["skipped"], 1);
        // Nothing was learned from the flag passed over.
        let doc = transcription_doc::read(&mut connection).await.expect("doc");
        assert!(
            doc.entries.iter().all(|entry| entry.written != "Sharath"),
            "{}",
            doc.body
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_meeting_too_long_for_the_model_fails_its_correction_for_good() {
        let (url, mut connection) = migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 4)
            .await
            .expect("a pool");
        let (stored, capture) = misheard_meeting(&mut connection).await;
        let step = CorrectTranscript::new(CorrectTranscriptDeps {
            database: database.clone(),
            events: EventBus::in_process(),
            runner: Runner::failing_with("Prompt is too long"),
        });
        let input = TranscribedMeeting {
            meeting_id: stored.id,
            capture_id: Some(capture.id),
            transcript_hash: Some("a".repeat(64)),
            recording: None,
            whisper_transcript_hash: Some("b".repeat(64)),
        };

        let failure = step
            .run(input, StepContext::for_test::<CorrectTranscript>(database))
            .await
            .expect_err("the model cannot read it");

        assert!(
            !failure.retryable,
            "every attempt would read the same prompt"
        );
        assert!(
            failure.reason.contains("Prompt is too long"),
            "{}",
            failure.reason
        );
        assert!(
            meeting_correction::find(&mut connection, stored.id)
                .await
                .expect("reads")
                .is_none(),
            "nothing is stored, for a person to skip the step"
        );
    }
}
