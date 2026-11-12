// Copyright © 2026 Jalapeno Labs

//! The meeting workflow, and where a meeting's run meets the meetings loop and a person.
//!
//! Every step of a meeting is a step of its run of the `meeting` workflow ([`workflow`]):
//! [`FetchMeeting`] fetches it (`pending` to `recorded`), [`TranscribeMeeting`] transcribes its
//! recording with the local Whisper, [`AlignMeeting`] puts its captures on one clock, lines their
//! words up, and may hold the run while its person says whether it is the same meeting as
//! another, [`CorrectTranscript`] corrects its transcript and may hold the run for its person's
//! approval, and [`SegmentMeeting`] cuts it into topics and publishes them to the timeline
//! (`segmented`, then `done`).
//!
//! A meeting whose captures change after its run aligned them (a merge, a split) is aligned again
//! by a run of the `meeting_alignment` workflow ([`alignment`]), the align step alone, which
//! leaves its processing where it was.
//!
//! # Where its state comes from
//!
//! A meeting keeps only how far its processing got. Whether it failed, and why, is its newest
//! run's, read as the meeting is answered ([`processing`]), so a step retried or skipped from the
//! Runs page moves the meeting at once, and nothing copies the run's state onto the meeting to
//! drift from it. A run that went through without publishing anything, because a person skipped
//! a step it needed, leaves the meeting skipped.
//!
//! # Which runs the loop starts
//!
//! The meetings loop (`crate::meetings`) starts a meeting's runs, started by the system user
//! ([`run_to_start`]): from the fetch for a meeting that never had one, and from segmenting for a
//! fetched meeting whose newest run ended before segmenting was one of its steps. A meeting with
//! any other run is that run's, live or not: a failed run waits for a person, and one a person
//! moved past leaves the meeting where they left it.

use std::collections::HashSet;

use chrono::Utc;
use diesel_async::AsyncPgConnection;
use uuid::Uuid;

use super::align::AlignMeeting;
use super::correct::{CorrectTranscript, CorrectedMeeting};
use super::fetch::{self, FetchMeeting, MeetingToFetch};
use super::segment::{SegmentMeeting, TranscriptToSegment};
use super::source::TranscriptLine;
use super::transcribe::{TranscribeMeeting, TranscribedMeeting};
use crate::models::meeting::{self, Meeting, MeetingState};
use crate::models::meeting_capture;
use crate::models::meeting_correction;
use crate::models::meeting_transcription;
use crate::models::step_job::{self, StepActionError, StepJobState};
use crate::models::user::SYSTEM_USER_ID;
use crate::models::workflow_run::{self, Actor, NewRun, WorkflowRun, WorkflowRunState};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::workflows::{self, Start, Step, Workflow};

/// What a meeting's run is about, as `workflow_runs.source_kind` names it.
pub const SOURCE_KIND: &str = "meeting";

/// The meeting workflow's name, as `workflow_runs.workflow` names it.
pub const NAME: &str = "meeting";

/// The meeting workflow: every step a meeting's run goes through, first to last.
pub fn workflow() -> Workflow<MeetingToFetch> {
    workflows::declare(NAME)
        .first::<FetchMeeting>()
        .then::<TranscribeMeeting>()
        .then::<AlignMeeting>()
        .then::<CorrectTranscript>()
        .then::<SegmentMeeting>()
        .build()
}

/// The meeting alignment workflow: a meeting's captures put on its clock and its transcript lined
/// up again, alone, for a meeting whose captures changed after its own run aligned them.
pub fn alignment() -> Workflow<TranscribedMeeting> {
    workflows::declare("meeting_alignment")
        .first::<AlignMeeting>()
        .build()
}

/// What the steps before aligning hand on for fetched meeting `fetched`, read from what it holds:
/// its first capture, that capture's transcript hash and recording, and its Whisper transcript.
async fn transcribed(
    connection: &mut AsyncPgConnection,
    fetched: &Meeting,
) -> anyhow::Result<TranscribedMeeting> {
    let capture = meeting_capture::first_of(connection, fetched.id).await?;
    let transcript_hash = if let Some(hash) = &capture.transcript_hash {
        hash.clone()
    } else {
        // A meeting fetched before transcripts were hashed: its transcript is what was stored.
        let lines: Vec<TranscriptLine> = serde_json::from_value(capture.transcript.clone())?;
        fetch::transcript_hash(&lines)
    };
    let whisper = meeting_transcription::find(connection, capture.id).await?;
    Ok(TranscribedMeeting {
        meeting_id: fetched.id,
        capture_id: Some(capture.id),
        transcript_hash: Some(transcript_hash),
        recording: capture.stored_recording(),
        whisper_transcript_hash: whisper.map(|stored| stored.transcript_hash),
    })
}

/// A run of the meeting alignment workflow ([`alignment`]) on fetched meeting `fetched`, started
/// by the system.
///
/// # Errors
/// Propagates any database error, and a stored transcript that does not read as its shape.
pub async fn run_alignment(
    connection: &mut AsyncPgConnection,
    fetched: &Meeting,
) -> anyhow::Result<NewRun> {
    let input = transcribed(connection, fetched).await?;
    Ok(alignment().new_run(Start {
        source_kind: SOURCE_KIND.to_owned(),
        source_id: fetched.id.to_string(),
        input,
        created_by: SYSTEM_USER_ID,
    }))
}

/// A run of the meeting workflow on fetched meeting `fetched`, entering it at aligning with what
/// the meeting holds, as the steps before would hand it on: for a meeting split back out of the
/// one it was merged into, whose own run went through while it was merged.
///
/// # Errors
/// Propagates any database error, and a stored transcript that does not read as its shape.
pub async fn run_from_aligning(
    connection: &mut AsyncPgConnection,
    fetched: &Meeting,
) -> anyhow::Result<NewRun> {
    let input = transcribed(connection, fetched).await?;
    Ok(workflow().new_run_at::<AlignMeeting>(Start {
        source_kind: SOURCE_KIND.to_owned(),
        source_id: fetched.id.to_string(),
        input,
        created_by: SYSTEM_USER_ID,
    }))
}

/// A run of the meeting workflow on meeting `meeting_id`, from its fetch, which Elysium starts
/// itself.
pub fn run_for(meeting_id: Uuid) -> NewRun {
    workflow().new_run(Start {
        source_kind: SOURCE_KIND.to_owned(),
        source_id: meeting_id.to_string(),
        input: MeetingToFetch {
            meeting_id,
            capture_id: None,
        },
        created_by: SYSTEM_USER_ID,
    })
}

/// A run of the meeting workflow on capture `capture_id` of meeting `meeting_id`, from its fetch,
/// which person `started_by` starts by finishing its upload.
pub fn run_for_upload(meeting_id: Uuid, capture_id: Uuid, started_by: Uuid) -> NewRun {
    workflow().new_run(Start {
        source_kind: SOURCE_KIND.to_owned(),
        source_id: meeting_id.to_string(),
        input: MeetingToFetch {
            meeting_id,
            capture_id: Some(capture_id),
        },
        created_by: started_by,
    })
}

/// Meeting `meeting_id`'s run that is still its, if it has one: going, held, or failed and waiting
/// for a person. A meeting holds one run at a time (`workflow_run::start`).
///
/// # Errors
/// Propagates any database error.
pub async fn live_run(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> diesel::QueryResult<Option<WorkflowRun>> {
    workflow_run::find_live(connection, NAME, SOURCE_KIND, &meeting_id.to_string()).await
}

/// A run of the meeting workflow on fetched meeting `fetched`, entering it at segmenting with
/// what the meeting holds: its capture's transcript, recording, and Whisper transcript, and its
/// corrected copy with whether it was approved, as the steps before segmenting would hand them on.
/// Started by `started_by`: the system for a meeting the loop carries on, or the person done
/// reviewing it (`super::review`).
///
/// # Errors
/// Propagates any database error, and a stored transcript that does not read as its shape.
pub async fn run_from_segmenting(
    connection: &mut AsyncPgConnection,
    fetched: &Meeting,
    started_by: Uuid,
) -> anyhow::Result<NewRun> {
    let transcribed = transcribed(connection, fetched).await?;
    let correction = meeting_correction::find(connection, fetched.id).await?;
    let input = CorrectedMeeting {
        meeting_id: transcribed.meeting_id,
        capture_id: transcribed.capture_id,
        transcript_hash: transcribed.transcript_hash,
        recording: transcribed.recording,
        whisper_transcript_hash: transcribed.whisper_transcript_hash,
        approved: correction
            .as_ref()
            .is_some_and(|correction| correction.approved_at.is_some()),
        corrected_transcript_hash: correction.map(|correction| correction.transcript_hash),
    };
    Ok(workflow().new_run_at::<SegmentMeeting>(Start {
        source_kind: SOURCE_KIND.to_owned(),
        source_id: fetched.id.to_string(),
        input: TranscriptToSegment::Meeting(input),
        created_by: started_by,
    }))
}

/// Which run the meetings loop starts for a meeting not yet done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunToStart {
    /// It never had a run: one from its fetch.
    FromFetch,
    /// It was fetched, and its newest run ended before segmenting was one of its steps: one
    /// that enters at segmenting.
    FromSegmenting,
}

/// The run the loop starts for unfinished meeting `state`, whose newest run is `newest`, which
/// names a segmenting step when `newest_segments`. `None` when its run carries it, or carried
/// it as far as a person let it.
pub fn run_to_start(
    state: MeetingState,
    newest: Option<WorkflowRunState>,
    newest_segments: bool,
) -> Option<RunToStart> {
    let fetched = matches!(state, MeetingState::Recorded | MeetingState::Segmented);
    match newest {
        None if state == MeetingState::Pending => Some(RunToStart::FromFetch),
        // A meeting fetched before it had a run of its own, or whose newest run succeeded
        // before segmenting was one of its steps.
        None | Some(WorkflowRunState::Succeeded) if fetched && !newest_segments => {
            Some(RunToStart::FromSegmenting)
        }
        // Done, or carried by its run as far as a person let it.
        _ => None,
    }
}

/// Where a meeting's processing stands, as its person sees it: how far it got, or its run's
/// failure, or that a person skipped past it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Processing {
    Pending,
    Recorded,
    Segmented,
    Done,
    /// Its newest run failed; the run says why.
    Failed,
    /// Its newest run went through without publishing it: a person skipped a step it needed.
    Skipped,
}

impl Processing {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Recorded => "recorded",
            Self::Segmented => "segmented",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

/// Where a meeting whose processing got to `state` stands, its newest run being `newest`, which
/// names a segmenting step when `newest_segments`.
pub fn processing(
    state: MeetingState,
    newest: Option<WorkflowRunState>,
    newest_segments: bool,
) -> Processing {
    match (newest, state) {
        (Some(WorkflowRunState::Failed), _) => Processing::Failed,
        (_, MeetingState::Done) => Processing::Done,
        (Some(WorkflowRunState::Succeeded), _) if newest_segments => Processing::Skipped,
        (_, MeetingState::Pending) => Processing::Pending,
        (_, MeetingState::Recorded) => Processing::Recorded,
        (_, MeetingState::Segmented) => Processing::Segmented,
    }
}

/// The runs among `runs` that segment, so [`processing`] and [`run_to_start`] can tell a run a
/// person skipped through from one that ended before segmenting was a step.
///
/// # Errors
/// Propagates any database error.
pub async fn segmenting_runs(
    connection: &mut AsyncPgConnection,
    runs: &[&WorkflowRun],
) -> diesel::QueryResult<HashSet<Uuid>> {
    let ids: Vec<Uuid> = runs.iter().map(|run| run.id).collect();
    workflow_run::naming_step(connection, &ids, SegmentMeeting::NAME).await
}

/// Tries meeting `id` again as `person`, its owner, whom the caller checked: its newest run's
/// failed step is queued again with a fresh set of attempts, as them, and the run goes on from
/// it. Announces both.
///
/// # Errors
/// [`StepActionError::Database`] with `NotFound` when the meeting does not exist or its newest
/// run has not failed; any other database error.
pub async fn retry(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    id: Uuid,
    person: Uuid,
) -> Result<Meeting, StepActionError> {
    let retried = meeting::find(connection, id).await?;
    let source_id = id.to_string();
    let newest = workflow_run::newest(connection, workflow().name(), SOURCE_KIND, &source_id)
        .await?
        .filter(|run| run.state == WorkflowRunState::Failed)
        .ok_or(diesel::result::Error::NotFound)?;
    let steps = workflow_run::steps_of(connection, newest.id).await?;
    let failed = steps
        .iter()
        .find(|step| step.state == StepJobState::Failed)
        .ok_or(diesel::result::Error::NotFound)?;
    let actor = Actor {
        user_id: person,
        at: Utc::now(),
    };
    let settled = step_job::retry(connection, newest.id, failed.id, actor).await?;

    workflows::announce(events, &settled);
    events.signal(&Signal::Workflows);
    events.publish(&ServerEvent::MeetingChanged {
        id: retried.id,
        state: retried.state.clone(),
        entry_id: None,
    });
    Ok(retried)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::models::meeting::tests::stored as stored_meeting;
    use crate::models::step_job::StepOutcome;
    use crate::test_support::{TEST_PERSON_ID, migrated_database};
    use crate::workflows::Done;

    #[test]
    fn the_engine_registers_every_step_the_meeting_workflow_names() {
        let manager = diesel_async::pooled_connection::AsyncDieselConnectionManager::new(
            "postgres://unused.invalid/none",
        );
        let unconnected = crate::database::Pool::builder(manager)
            .build()
            .expect("a pool builds without connecting");
        let cipher = std::sync::Arc::new(crate::test_support::cipher());
        let mut steps = crate::workflows::Steps::new();
        steps.register(FetchMeeting::for_test(
            unconnected.clone(),
            std::sync::Arc::clone(&cipher),
            crate::meetings::sources::Sources::new(reqwest::Client::new()),
        ));
        steps.register(TranscribeMeeting::for_test(unconnected.clone(), cipher));
        steps.register(AlignMeeting::for_test(unconnected.clone()));
        steps.register(CorrectTranscript::for_test(unconnected.clone()));
        steps.register(SegmentMeeting::for_test(unconnected));
        // Panics, as the engine would at startup, if the two drift apart.
        steps.assert_runs(&workflow().outline());
        steps.assert_runs(&alignment().outline());
    }

    #[test]
    fn a_meetings_run_starts_by_fetching_it_and_is_about_that_meeting() {
        let meeting_id = Uuid::now_v7();
        let run = run_for(meeting_id);

        assert_eq!(run.workflow, "meeting");
        assert_eq!(
            run.steps,
            vec![
                "fetch_meeting".to_owned(),
                "transcribe_meeting".to_owned(),
                "align_meeting".to_owned(),
                "correct_transcript".to_owned(),
                "segment_meeting".to_owned(),
            ]
        );
        assert_eq!(
            (run.source_kind.as_str(), run.source_id),
            (SOURCE_KIND, meeting_id.to_string())
        );
        assert_eq!(run.input, json!({ "meetingId": meeting_id }));
        assert_eq!(run.created_by, SYSTEM_USER_ID);
    }

    #[test]
    fn the_loop_starts_a_run_only_for_a_meeting_no_run_carries() {
        use MeetingState::{Done, Pending, Recorded, Segmented};
        use WorkflowRunState::{Failed, Held, Queued, Running, Succeeded};

        assert_eq!(
            run_to_start(Pending, None, false),
            Some(RunToStart::FromFetch)
        );
        // Fetched before it had a run of its own, or before segmenting was a step of its run.
        assert_eq!(
            run_to_start(Recorded, None, false),
            Some(RunToStart::FromSegmenting)
        );
        assert_eq!(
            run_to_start(Segmented, Some(Succeeded), false),
            Some(RunToStart::FromSegmenting)
        );
        // A run under way, waiting, or failed carries it; a person moves it on from there.
        for state in [Queued, Running, Held, Failed] {
            assert_eq!(run_to_start(Pending, Some(state), true), None);
            assert_eq!(run_to_start(Recorded, Some(state), true), None);
        }
        // A run a person skipped through leaves the meeting where they left it: skipping the
        // fetch skips the meeting, rather than starting another run that fetches it.
        assert_eq!(run_to_start(Pending, Some(Succeeded), true), None);
        assert_eq!(run_to_start(Recorded, Some(Succeeded), true), None);
        assert_eq!(run_to_start(Done, None, false), None);
    }

    #[test]
    fn a_meetings_processing_reads_its_newest_runs_failure_and_skips() {
        use MeetingState::{Done, Pending, Recorded, Segmented};
        use WorkflowRunState::{Failed, Held, Running, Succeeded};

        assert_eq!(processing(Recorded, Some(Failed), true), Processing::Failed);
        assert_eq!(processing(Pending, Some(Failed), true), Processing::Failed);
        // Retried from the Runs page, it reads as the run going again, at once.
        assert_eq!(
            processing(Pending, Some(Running), true),
            Processing::Pending
        );
        assert_eq!(processing(Recorded, Some(Held), true), Processing::Recorded);
        assert_eq!(processing(Done, Some(Succeeded), true), Processing::Done);
        assert_eq!(
            processing(Pending, Some(Succeeded), true),
            Processing::Skipped
        );
        assert_eq!(
            processing(Recorded, Some(Succeeded), true),
            Processing::Skipped
        );
        // A run from before segmenting was a step leaves it for the run that segments it.
        assert_eq!(
            processing(Segmented, Some(Succeeded), false),
            Processing::Segmented
        );
        assert_eq!(processing(Done, None, false), Processing::Done);
    }

    /// Claims the run's next step, which must be `Running`, and runs one attempt of `step` on
    /// the input its row holds, as the engine's workers do, recording how it ended.
    async fn run_next<Running: Step>(
        connection: &mut AsyncPgConnection,
        step: &Running,
        database: &crate::database::Pool,
    ) -> Done<Running::Output> {
        let at = Utc::now();
        let claims = step_job::claim(connection, at, 1)
            .await
            .expect("claims")
            .claims;
        let claim = claims.first().expect("a step is due");
        assert_eq!(claim.job.step, Running::NAME);
        let input = serde_json::from_value(claim.job.input.clone().expect("an input"))
            .expect("the step reads its input");
        let context = crate::workflows::StepContext::for_test::<Running>(database.clone());
        let done = step.run(input, context).await.expect("an attempt");
        let outcome = match &done {
            Done::Succeeded { output, summary } => StepOutcome::Succeeded {
                output: serde_json::to_value(output).expect("serializes"),
                summary: summary.clone(),
            },
            Done::Held { output, summary } => StepOutcome::Held {
                output: serde_json::to_value(output).expect("serializes"),
                summary: summary.clone(),
            },
            Done::Skipped { reason } => StepOutcome::Skipped {
                reason: reason.clone(),
            },
        };
        step_job::finish(connection, claim, outcome, at)
            .await
            .expect("records")
            .expect("the claim holds");
        done
    }

    /// #77: a person skipping a meeting's fetch on the Runs page skipped the step but not the
    /// meeting, and its next pass started another run that fetched it.
    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn issue_77_skipping_the_fetch_skips_the_meeting() {
        let (url, mut connection) = migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 4)
            .await
            .expect("a pool");
        let events = EventBus::in_process();
        let (pending, _capture) = stored_meeting(&mut connection).await;
        let started = workflows::start(&mut connection, &events, run_for(pending.id))
            .await
            .expect("the run starts");
        let run_id = started.settled.run.id;
        let fetch = workflow_run::steps_of(&mut connection, run_id)
            .await
            .expect("steps")
            .remove(0);
        let actor = Actor {
            user_id: TEST_PERSON_ID,
            at: Utc::now(),
        };
        step_job::skip(&mut connection, run_id, fetch.id, actor)
            .await
            .expect("skipped");

        // Every step after it receives the fetch's own input, and skips the meeting too.
        let cipher = std::sync::Arc::new(crate::test_support::cipher());
        let transcribe = TranscribeMeeting::for_test(database.clone(), cipher);
        let transcribed = run_next(&mut connection, &transcribe, &database).await;
        assert!(matches!(transcribed, Done::Skipped { .. }));
        let align = AlignMeeting::for_test(database.clone());
        let aligned = run_next(&mut connection, &align, &database).await;
        assert!(matches!(aligned, Done::Skipped { .. }));
        let correct = CorrectTranscript::for_test(database.clone());
        let corrected = run_next(&mut connection, &correct, &database).await;
        assert!(matches!(corrected, Done::Skipped { .. }));
        let segment = SegmentMeeting::for_test(database.clone());
        let segmented = run_next(&mut connection, &segment, &database).await;
        assert_eq!(
            segmented,
            Done::Skipped {
                reason: "Not fetched, so nothing to segment".to_owned(),
            }
        );

        let run = workflow_run::find(&mut connection, run_id)
            .await
            .expect("the run");
        assert_eq!(run.state, WorkflowRunState::Succeeded);
        let segments = segmenting_runs(&mut connection, &[&run])
            .await
            .expect("reads")
            .contains(&run.id);
        let stored = meeting::find(&mut connection, pending.id)
            .await
            .expect("found");
        assert_eq!(
            processing(stored.state(), Some(run.state), segments),
            Processing::Skipped,
            "the meeting reads as skipped"
        );
        assert_eq!(
            run_to_start(stored.state(), Some(run.state), segments),
            None,
            "and no pass starts another run that fetches it"
        );
    }

    /// A meeting fetched before segmenting was a step of its run is carried on by a run that
    /// enters at segmenting, with what the meeting holds, as the steps before would hand it on.
    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_meeting_fetched_before_segmenting_was_a_step_enters_its_run_there() {
        use diesel::{ExpressionMethods, QueryDsl};
        use diesel_async::RunQueryDsl;

        use crate::meetings::correction;
        use crate::models::meeting_capture::Fetched;
        use crate::models::meeting_correction::{self, NewMeetingCorrection};

        let (_url, mut connection) = migrated_database().await;
        let events = EventBus::in_process();
        let heard = vec![TranscriptLine {
            start_ms: 0,
            end_ms: 3_000,
            speaker: "Ada".to_owned(),
            text: "Jon ships Friday".to_owned(),
        }];
        let (pending, pending_capture) = stored_meeting(&mut connection).await;
        let captured = meeting_capture::record_fetched(
            &mut connection,
            pending_capture.id,
            Fetched {
                transcript: serde_json::to_value(&heard).expect("serializes"),
                transcript_hash: fetch::transcript_hash(&heard),
                recording: None,
            },
        )
        .await
        .expect("records")
        .expect("the first fetch");
        let fetched = meeting::record_recorded(&mut connection, pending.id)
            .await
            .expect("moves");
        let mut corrected = heard.clone();
        corrected[0].text = "John ships Friday".to_owned();
        let corrected_hash = correction::transcript_hash(&corrected);
        meeting_correction::record(
            &mut connection,
            NewMeetingCorrection {
                meeting_id: fetched.id,
                transcript: serde_json::to_value(&corrected).expect("serializes"),
                transcript_hash: corrected_hash.clone(),
                based_on: "source".to_owned(),
                source_transcript_hash: fetch::transcript_hash(&heard),
                whisper_transcript_hash: None,
                changed_lines: 1,
                fixes: json!([]),
            },
        )
        .await
        .expect("records")
        .expect("new");
        meeting_correction::approve(&mut connection, fetched.id, TEST_PERSON_ID, Utc::now())
            .await
            .expect("approved");

        let new_run = run_from_segmenting(&mut connection, &fetched, SYSTEM_USER_ID)
            .await
            .expect("builds");
        assert_eq!(new_run.steps, vec!["segment_meeting".to_owned()]);
        let input: TranscriptToSegment =
            serde_json::from_value(new_run.input.clone()).expect("reads");
        let TranscriptToSegment::Meeting(input) = input else {
            panic!("a meeting enters as a meeting, not {input:?}");
        };
        assert_eq!(input.meeting_id, fetched.id);
        assert_eq!(input.capture_id, Some(captured.id));
        assert_eq!(input.transcript_hash, captured.transcript_hash);
        assert_eq!(input.corrected_transcript_hash, Some(corrected_hash));
        assert!(input.approved);

        // Once started, the run carries the meeting: the loop starts no other.
        let started = workflows::start(&mut connection, &events, new_run)
            .await
            .expect("starts");
        let run = started.settled.run;
        let segments = segmenting_runs(&mut connection, &[&run])
            .await
            .expect("reads")
            .contains(&run.id);
        assert!(segments);
        assert_eq!(
            run_to_start(fetched.state(), Some(run.state), segments),
            None
        );

        // A meeting fetched before transcripts were hashed enters with its stored transcript's hash.
        diesel::update(crate::database::schema::meeting_captures::table.find(captured.id))
            .set(crate::database::schema::meeting_captures::transcript_hash.eq(None::<String>))
            .execute(&mut connection)
            .await
            .expect("unhashes");
        let unhashed_run = run_from_segmenting(&mut connection, &fetched, SYSTEM_USER_ID)
            .await
            .expect("builds");
        assert_eq!(
            unhashed_run.input["transcriptHash"],
            json!(fetch::transcript_hash(&heard))
        );
    }

    /// Fails the run's next step for good, as a source refusing the key would.
    async fn fail_next(connection: &mut AsyncPgConnection) {
        let at = Utc::now();
        let claims = step_job::claim(connection, at, 1)
            .await
            .expect("claims")
            .claims;
        step_job::finish(
            connection,
            &claims[0],
            StepOutcome::Failed {
                reason: "Fireflies refused the key".to_owned(),
                retryable: false,
            },
            at,
        )
        .await
        .expect("records")
        .expect("the claim holds");
    }

    /// #77, too: retrying a meeting's failed step from the Runs page left the Meetings page
    /// showing the failure, since the meeting kept a copy of it. Retrying from either page now
    /// moves the meeting at once.
    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn issue_77_retrying_from_either_page_moves_the_meeting_at_once() {
        let (_url, mut connection) = migrated_database().await;
        let events = EventBus::in_process();
        let (pending, _capture) = stored_meeting(&mut connection).await;

        // The loop starts the meeting's run, and its fetch fails for good.
        let started = workflows::start(&mut connection, &events, run_for(pending.id))
            .await
            .expect("the run starts");
        let run_id = started.settled.run.id;
        fail_next(&mut connection).await;
        let failed = workflow_run::find(&mut connection, run_id)
            .await
            .expect("the run");
        assert_eq!(
            processing(pending.state(), Some(failed.state), true),
            Processing::Failed
        );

        // Retried from the Runs page: the meeting reads as its run going again.
        let fetch = workflow_run::steps_of(&mut connection, run_id)
            .await
            .expect("steps")
            .remove(0);
        let actor = Actor {
            user_id: TEST_PERSON_ID,
            at: Utc::now(),
        };
        step_job::retry(&mut connection, run_id, fetch.id, actor)
            .await
            .expect("retried");
        let going = workflow_run::find(&mut connection, run_id)
            .await
            .expect("the run");
        assert_eq!(
            processing(pending.state(), Some(going.state), true),
            Processing::Pending
        );

        // It fails again, and is retried from the Meetings page this time.
        fail_next(&mut connection).await;

        // Its person retries it, whom its route checks (`crate::meetings::access`), and its run's
        // fetch is queued again as them.
        retry(&mut connection, &events, pending.id, TEST_PERSON_ID)
            .await
            .expect("retried");
        let run = workflow_run::find(&mut connection, started.settled.run.id)
            .await
            .expect("the run");
        assert_ne!(run.state, WorkflowRunState::Failed);
        assert_ne!(
            processing(pending.state(), Some(run.state), true),
            Processing::Failed,
            "the meeting reads as its run going again, with nothing copied onto it"
        );
        let steps = workflow_run::steps_of(&mut connection, run.id)
            .await
            .expect("steps");
        assert_eq!(
            (steps[0].state, steps[0].attempts),
            (StepJobState::Queued, 0)
        );
        let history = workflow_run::history(&mut connection, run.id)
            .await
            .expect("history");
        let last = history.last().expect("an entry");
        assert_eq!(
            (last.kind.as_str(), last.created_by),
            ("retried", TEST_PERSON_ID)
        );

        // With nothing failed, there is nothing to retry.
        assert!(matches!(
            retry(&mut connection, &events, pending.id, TEST_PERSON_ID).await,
            Err(StepActionError::Database(diesel::result::Error::NotFound))
        ));
        // And the run it has carries it: the loop starts no other.
        let segments = segmenting_runs(&mut connection, &[&run])
            .await
            .expect("reads");
        assert_eq!(
            run_to_start(pending.state(), Some(run.state), segments.contains(&run.id)),
            None
        );
    }
}
