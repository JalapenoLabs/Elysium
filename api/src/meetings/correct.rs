// Copyright © 2026 Jalapeno Labs

//! The meeting workflow's third step, [`CorrectTranscript`]: a corrected copy of the meeting's
//! transcript, its misheard names and terms fixed against every transcript the meeting has,
//! the company doc, and the company's other meetings, beside the raw ones it was made from.
//!
//! The prompt and the checks on the model's answer are `super::correction`; this module runs
//! the turns, stores the copy, lines the meeting's final transcript up again with it
//! (`super::final_transcript`), and decides whether the run waits for its person's approval.
//!
//! # Doing it once
//!
//! A meeting is corrected once. The raw transcripts are never written: the copy goes in
//! `meeting_corrections` (`models::meeting_correction`), which keeps the first copy stored, and
//! running the step again answers it without asking the model anything. Every attempt that has a
//! copy lines the final transcript up with it, which changes nothing once it has, so an engine
//! that stopped between the two does it on the next.
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
//! carries counts, line numbers, sections of the company doc, and hashes, never a word of what
//! was said.

use std::sync::Arc;

use anyhow::anyhow;
use chrono::Utc;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::correction::{self, Background, Base, Edit};
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
use crate::models::meeting_capture::{self, MeetingCapture, StoredRecording};
use crate::models::meeting_correction::{self, MeetingCorrection, NewMeetingCorrection};
use crate::models::meeting_event::{self, Change, EventKind};
use crate::models::meeting_transcription;
use crate::models::model_setting::ModelChoice;
use crate::models::step_job::{self, StepActionError, StepJobState};
use crate::models::user::SYSTEM_USER_ID;
use crate::models::workflow_run::{self, Settled};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::routes::v1::action_items::publish_item_write;
use crate::runner::events::{RunnerEvent, TurnStatus};
use crate::runner::{ConversationId, Runner, TurnMode, TurnRequest, WorkspacePath};
use crate::whisper::WhisperTranscript;
use crate::workflows::{self, Done, Step, StepContext, StepFailure};

/// Where correction turns run in the workspace. Text turns read and write no files.
const CORRECTION_DIRECTORY: &str = ".elysium/correction";

/// How many of the company's newest corrected meetings a correction learns fixes from, and how
/// many meetings it reads speakers' names from: enough to know the company's recurring names
/// and terms, few enough to read in one query.
const OTHER_MEETINGS: i32 = 30;

/// The most speakers' names a prompt lists.
const SPEAKERS_MAX: i32 = 200;

/// The longest title of an inbox item, as items allow.
const ITEM_TITLE_MAX_CHARACTERS: usize = 500;

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

/// A meeting's transcripts and everything its correction reads beside them.
struct Heard {
    meeting: Meeting,
    base: Base,
    whisper: Option<WhisperTranscript>,
    doc: company_doc::CompanyDocResponse,
    fixes: Vec<correction::Fix>,
    speakers: Vec<String>,
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

    /// The meeting's transcripts, and what its correction reads beside them.
    async fn read(&self, input: &TranscribedMeeting) -> Result<Read, StepFailure> {
        let mut connection = self.connection().await?;
        let read = async {
            let meeting = meeting::find(&mut connection, input.meeting_id).await?;
            let read_whisper = input.whisper_transcript_hash.is_some();
            let (_capture, base, whisper) = heard_by(&mut connection, input, read_whisper).await?;
            let Some(base) = base else {
                return anyhow::Ok(Read::Silent);
            };
            let doc = company_doc::read(&mut connection).await?;
            let stored_fixes =
                meeting_correction::recent_fixes(&mut connection, meeting.id, OTHER_MEETINGS)
                    .await?;
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
                whisper,
                doc,
                fixes: correction::learned_fixes(&stored_fixes),
                speakers,
            })))
        };
        read.await.map_err(StepFailure::retryable)
    }

    /// Corrects `heard` in as many turns as it takes, and stores the copy. Answers the copy
    /// stored, which is another attempt's when one stored it first.
    async fn correct(
        &self,
        heard: &Heard,
        input: &TranscribedMeeting,
        source_transcript_hash: &str,
        context: &StepContext,
    ) -> Result<MeetingCorrection, StepFailure> {
        let participants: Vec<String> =
            serde_json::from_value(heard.meeting.participants.clone()).unwrap_or_default();
        let background = correction::background(Background {
            title: &heard.meeting.title,
            participants: &participants,
            doc: &heard.doc,
            fixes: &heard.fixes,
            speakers: &heard.speakers,
        });
        for section in &background.sections_cut {
            context.log.line(&format!(
                "The company doc's {section} section did not fit whole beside the transcript"
            ));
        }
        let lines = &heard.base.lines;
        let room = correction::transcript_room(&background.text);
        let mut turns = correction::plan_turns(room, lines, heard.whisper.as_ref());
        context.log.line(&format!(
            "Correcting {} lines of the {} transcript{}, in {} turns",
            lines.len(),
            heard.base.based_on.as_str(),
            if heard.whisper.is_some() {
                ", beside Whisper's"
            } else {
                ""
            },
            turns.len().min(correction::TURNS_MAX)
        ));
        if turns.len() > correction::TURNS_MAX {
            let left_from = turns[correction::TURNS_MAX].start;
            context.log.line(&format!(
                "Lines from {left_from} on are left as heard: the meeting takes more than {} turns",
                correction::TURNS_MAX
            ));
            turns.truncate(correction::TURNS_MAX);
        }

        let model = context.model().await?;
        let mut edits: Vec<Edit> = Vec::new();
        for turn in turns {
            let prompt = correction::prompt(
                &background.text,
                lines,
                turn.clone(),
                heard.whisper.as_ref(),
            );
            let answer = self.ask(heard.meeting.id, prompt, &model).await?;
            let checked =
                correction::parse_answer(&answer, lines, turn.clone()).map_err(|error| {
                    StepFailure::retryable(format!("The correction's answer: {error}"))
                })?;
            context.log.line(&format!(
                "Lines {} to {}: {} corrected, {} edits refused",
                turn.start,
                turn.end - 1,
                checked.edits.len(),
                checked.refused.len()
            ));
            for (line, refusal) in &checked.refused {
                context
                    .log
                    .line(&format!("Refused the edit to line {line}: {refusal}"));
            }
            edits.extend(checked.edits);
        }

        let corrected = correction::apply(lines, &edits);
        let hash = correction::transcript_hash(&corrected);
        let new = NewMeetingCorrection {
            meeting_id: heard.meeting.id,
            transcript: serde_json::to_value(&corrected).map_err(StepFailure::permanent)?,
            transcript_hash: hash.clone(),
            based_on: heard.base.based_on.as_str().to_owned(),
            source_transcript_hash: source_transcript_hash.to_owned(),
            whisper_transcript_hash: input.whisper_transcript_hash.clone(),
            changed_lines: i32::try_from(edits.len()).unwrap_or(i32::MAX),
            fixes: serde_json::to_value(correction::distinct_fixes(&edits))
                .map_err(StepFailure::permanent)?,
        };
        let mut connection = self.connection().await?;
        let written = meeting_correction::record(&mut connection, new)
            .await
            .map_err(StepFailure::retryable)?;
        if let Some(written) = written {
            context
                .log
                .line(&format!("Stored the corrected copy, SHA-256 {hash}"));
            return Ok(written);
        }
        // Another attempt stored its copy first; that one is what every later step reads.
        let stored = meeting_correction::find(&mut connection, heard.meeting.id)
            .await
            .map_err(StepFailure::retryable)?
            .ok_or_else(|| StepFailure::retryable("The meeting's corrected copy went missing"))?;
        context.log.line(&format!(
            "Another attempt stored its corrected copy first, SHA-256 {}; it is kept",
            stored.transcript_hash
        ));
        Ok(stored)
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
        if outcome.status != TurnStatus::Completed {
            return Err(StepFailure::retryable(anyhow!(
                "The correction turn ended {:?}: {}",
                outcome.status,
                outcome.error.unwrap_or_default()
            )));
        }
        Ok(outcome.result.unwrap_or_default())
    }

    /// Lines meeting `meeting_id`'s final transcript up again with its corrected copy, which
    /// changes nothing once it has (`final_transcript::rebuild`).
    async fn build_final_transcript(
        &self,
        meeting_id: Uuid,
        log: &crate::workflows::StepLog,
    ) -> Result<(), StepFailure> {
        let mut connection = self.connection().await?;
        let author = Author {
            user_id: SYSTEM_USER_ID,
            agent: None,
            at: Utc::now(),
        };
        let built = final_transcript::rebuild(&mut connection, meeting_id, author)
            .await
            .map_err(StepFailure::retryable)?;
        if let Some(built) = built {
            log.line(&format!(
                "Lined the final transcript up with the corrected copy: {} words written, {} \
                 replaced",
                built.data["added"], built.data["replaced"]
            ));
        }
        Ok(())
    }

    /// Whether the run waits for the meeting's person, decided under its lock, raising the
    /// inbox item that asks for the review when it waits and none was raised.
    async fn decide(&self, meeting_id: Uuid) -> Result<Decision, StepFailure> {
        let mut connection = self.connection().await?;
        let (decision, raised) = connection
            .transaction(async move |connection| {
                let current = meeting::lock(connection, meeting_id).await?;
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

    fn announce(&self, changed: &Meeting) {
        self.inner.events.publish(&ServerEvent::MeetingChanged {
            id: changed.id,
            state: changed.state.clone(),
            created_by: changed.created_by,
        });
    }
}

impl Step for CorrectTranscript {
    const NAME: &'static str = "correct_transcript";
    /// It reads the transcripts and corrects them in agent turns.
    const AGENT_KIND: Option<AgentKind> = Some(AgentKind::Correction);
    type Input = TranscribedMeeting;
    type Output = TranscriptToSegment;

    async fn run(
        &self,
        input: TranscribedMeeting,
        context: StepContext,
    ) -> Result<Done<TranscriptToSegment>, StepFailure> {
        let Some(source_transcript_hash) = input.transcript_hash.clone() else {
            context.log.line("The meeting's fetch was skipped");
            return Ok(Done::Skipped {
                reason: "Not fetched, so nothing to correct".to_owned(),
            });
        };
        let (merged, stored) = {
            let mut connection = self.connection().await?;
            let current = meeting::find(&mut connection, input.meeting_id)
                .await
                .map_err(StepFailure::retryable)?;
            let stored = meeting_correction::find(&mut connection, input.meeting_id)
                .await
                .map_err(StepFailure::retryable)?;
            (current.merged_into.is_some(), stored)
        };
        if merged {
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
            let Read::Heard(heard) = self.read(&input).await? else {
                context.log.line("Neither transcript holds anything said");
                return Ok(Done::Skipped {
                    reason: "Nothing was said, so nothing to correct".to_owned(),
                });
            };
            let stored = self
                .correct(&heard, &input, &source_transcript_hash, &context)
                .await?;
            self.announce(&heard.meeting);
            stored
        };
        self.build_final_transcript(input.meeting_id, &context.log)
            .await?;

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

/// The capture `input` names, and what it heard: the transcript a correction follows
/// (`correction::base`), `None` when nothing was heard, and Whisper's transcript, read when
/// `read_whisper` and there is one.
async fn heard_by(
    connection: &mut AsyncPgConnection,
    input: &TranscribedMeeting,
    read_whisper: bool,
) -> anyhow::Result<(MeetingCapture, Option<Base>, Option<WhisperTranscript>)> {
    let capture = super::capture_of(connection, input.meeting_id, input.capture_id).await?;
    let source: Vec<TranscriptLine> = serde_json::from_value(capture.transcript.clone())
        .map_err(|error| anyhow!("the capture's transcript is not in the stored shape: {error}"))?;
    let whisper = if read_whisper {
        meeting_transcription::find(connection, capture.id)
            .await?
            .map(|stored| serde_json::from_value::<WhisperTranscript>(stored.transcript))
            .transpose()
            .map_err(|error| {
                anyhow!("the capture's Whisper transcript is not in the stored shape: {error}")
            })?
    } else {
        None
    };
    let base = correction::base(source, whisper.as_ref());
    Ok((capture, base, whisper))
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
/// [`ApproveError::NotFound`] when the meeting is not `owner`'s or has no corrected copy; any
/// database error.
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
/// its copy is stored; any database error.
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
    use crate::models::meeting_word::{self, Origin};
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
        // The attempt built the final transcript from the stored copy, its fix the correction's.
        let words = meeting_word::standing(&mut connection, stored.id)
            .await
            .expect("reads");
        assert_eq!(meeting_word::text_of(&words), "John is here");
        assert!(
            words
                .iter()
                .all(|word| word.origin == Origin::Correction.as_str())
        );
        assert!(words.iter().all(|word| word.capture_id == Some(capture.id)));
        assert_eq!(
            words
                .iter()
                .map(|word| (word.start_ms, word.end_ms))
                .collect::<Vec<_>>(),
            vec![(61_000, 62_000), (62_000, 63_000), (63_000, 64_000)],
            "on the meeting's clock, the line's time spread across its words"
        );
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

        // Its history: held once, built once (however many attempts ran), approved once.
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
                ("built", crate::models::user::SYSTEM_USER_ID),
                ("correction_approved", TEST_PERSON_ID),
            ]
        );
        assert_eq!(history[0].data, json!({ "before": false, "after": true }));
        assert_eq!(
            history[1].data,
            json!({
                "captures": [{ "captureId": capture.id, "offsetMs": 0 }],
                "builtFrom": "correction",
                "words": 3,
                "added": 3,
                "replaced": 0,
                "marksMoved": 0,
            })
        );

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
        let (stored, capture) = heard_meeting(&mut connection, "Standup").await;
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
        // A copy that changed nothing builds a transcript as its source heard it.
        let words = meeting_word::standing(&mut connection, stored.id)
            .await
            .expect("reads");
        assert_eq!(meeting_word::text_of(&words), "Jon is here");
        assert!(
            words
                .iter()
                .all(|word| word.origin == Origin::Source.as_str())
        );
        assert!(words.iter().all(|word| word.capture_id == Some(capture.id)));

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
}
