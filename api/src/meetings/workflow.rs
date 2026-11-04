// Copyright © 2026 Jalapeno Labs

//! The meeting workflow, and where a meeting's run meets the meetings loop and a person's retry.
//!
//! A meeting is carried in two halves while the engine's meeting steps are built out:
//!
//! - **Its run** of the `meeting` workflow ([`workflow`]), one per meeting, fetches it
//!   ([`FetchMeeting`]), which moves it from `pending` to `recorded`, transcribes its recording
//!   with the local Whisper ([`TranscribeMeeting`]), and corrects its transcript
//!   ([`CorrectTranscript`]), which may hold the run for its person's approval. The loop starts
//!   the run for each `pending` meeting on its processing pass ([`run_for`]), started by the
//!   system user, and a run whose fetch fails fails the meeting with its reason, so the
//!   Meetings page and its connector say why.
//! - **The loop** (`crate::meetings`) segments a `recorded` meeting with one of Elysia's text
//!   turns and publishes its segments, once its run is through, from the corrected copy the run
//!   handed on ([`corrected_copy`]), or the source's transcript when it handed on none. A run
//!   that failed or is held keeps the meeting waiting for a person.
//!
//! Segmenting is on the roadmap as a step after the correction, receiving the corrected copy as
//! its input and replacing the loop's half. See `docs/meetings.md`, Processing.

use chrono::Utc;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use tracing::{Level, event};
use uuid::Uuid;

use super::correct::{self, CorrectTranscript, CorrectedMeeting};
use super::fetch::{FetchMeeting, MeetingToFetch};
use super::source::TranscriptLine;
use super::transcribe::TranscribeMeeting;
use crate::models::meeting::{self, Meeting, MeetingState};
use crate::models::step_job::{self, StepActionError, StepJobState};
use crate::models::user::SYSTEM_USER_ID;
use crate::models::workflow_run::{self, Actor, NewRun, Settled};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::workflows::{self, Start, Step, Workflow};

/// What a meeting's run is about, as `workflow_runs.source_kind` names it.
pub const SOURCE_KIND: &str = "meeting";

/// The meeting workflow: every step a meeting's run goes through, first to last.
pub fn workflow() -> Workflow<MeetingToFetch> {
    workflows::declare("meeting")
        .first::<FetchMeeting>()
        .then::<TranscribeMeeting>()
        .then::<CorrectTranscript>()
        .build()
}

/// The corrected copy meeting `meeting_id`'s newest run handed on past its correction, when it
/// corrected the meeting: what the loop segments. `None` for a run that has not passed its
/// correction, skipped it, or ran before the workflow corrected meetings, which leaves the
/// source's transcript to segment.
///
/// # Errors
/// Propagates any database error, and a stored output or copy that does not read as its shape.
pub async fn corrected_copy(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> anyhow::Result<Option<Vec<TranscriptLine>>> {
    let source_id = meeting_id.to_string();
    let newest =
        workflow_run::newest(connection, workflow().name(), SOURCE_KIND, &source_id).await?;
    let Some(run) = newest else {
        return Ok(None);
    };
    let steps = workflow_run::steps_of(connection, run.id).await?;
    let handed_on = steps
        .into_iter()
        .find(|step| step.step == CorrectTranscript::NAME && step.state == StepJobState::Succeeded)
        .and_then(|step| step.output);
    let Some(output) = handed_on else {
        return Ok(None);
    };
    let corrected: CorrectedMeeting = serde_json::from_value(output)?;
    correct::corrected_lines(connection, &corrected).await
}

/// A run of the meeting workflow on meeting `meeting_id`, which Elysium starts itself.
pub fn run_for(meeting_id: Uuid) -> NewRun {
    workflow().new_run(Start {
        source_kind: SOURCE_KIND.to_owned(),
        source_id: meeting_id.to_string(),
        input: MeetingToFetch { meeting_id },
        created_by: SYSTEM_USER_ID,
    })
}

/// Tries failed meeting `id` of `owner` again from the step it failed at. One whose fetch failed
/// also has its run's failed step retried, as `owner`: its run is still live, so without that the
/// loop would find the same failed run and fail the meeting again. Announces both.
///
/// # Errors
/// [`StepActionError::Database`] with `NotFound` when the meeting does not exist, belongs to
/// someone else, or has not failed; any other database error.
pub async fn retry(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    id: Uuid,
    owner: Uuid,
) -> Result<Meeting, StepActionError> {
    let (retried, settled) = connection
        .transaction(async move |connection| {
            let retried = meeting::retry(connection, id, owner).await?;
            let settled = if retried.state() == MeetingState::Pending {
                retry_failed_fetch(connection, id, owner).await?
            } else {
                None
            };
            Ok::<_, StepActionError>((retried, settled))
        })
        .await?;

    if let Some(settled) = settled {
        workflows::announce(events, &settled);
        events.signal(&Signal::Workflows);
    }
    events.publish(&ServerEvent::MeetingChanged {
        id: retried.id,
        state: retried.state.clone(),
        created_by: retried.created_by,
    });
    Ok(retried)
}

/// Queues meeting `id`'s failed fetch again, when its live run failed.
async fn retry_failed_fetch(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    owner: Uuid,
) -> Result<Option<Settled>, StepActionError> {
    let source_id = id.to_string();
    let live =
        workflow_run::find_live(connection, workflow().name(), SOURCE_KIND, &source_id).await?;
    let Some(run) = live else {
        event!(
            name: "meetings.retry.no_run",
            Level::DEBUG,
            meeting.id = %id,
            "a retried meeting has no live run; the next processing pass starts one",
        );
        return Ok(None);
    };
    let steps = workflow_run::steps_of(connection, run.id).await?;
    let Some(failed) = steps.iter().find(|step| step.state == StepJobState::Failed) else {
        event!(
            name: "meetings.retry.run_not_failed",
            Level::DEBUG,
            meeting.id = %id,
            workflow_run.id = %run.id,
            "a retried meeting's run has no failed step, so it goes on as it is",
        );
        return Ok(None);
    };
    let actor = Actor {
        user_id: owner,
        at: Utc::now(),
    };
    Ok(Some(
        step_job::retry(connection, run.id, failed.id, actor).await?,
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::models::meeting::NewMeeting;
    use crate::models::step_job::StepOutcome;
    use crate::models::workflow_run::WorkflowRunState;
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

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
        steps.register(CorrectTranscript::for_test(unconnected));
        // Panics, as the engine would at startup, if the two drift apart.
        steps.assert_runs(&workflow().outline());
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
                "correct_transcript".to_owned(),
            ]
        );
        assert_eq!(
            (run.source_kind.as_str(), run.source_id),
            (SOURCE_KIND, meeting_id.to_string())
        );
        assert_eq!(run.input, serde_json::json!({ "meetingId": meeting_id }));
        assert_eq!(run.created_by, SYSTEM_USER_ID);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn retrying_a_meeting_whose_fetch_failed_retries_its_run_rather_than_failing_again() {
        let (_url, mut connection) = migrated_database().await;
        let events = EventBus::in_process();
        let pending = meeting::insert_new(
            &mut connection,
            NewMeeting {
                id: Uuid::now_v7(),
                connector_id: None,
                source_kind: "fireflies".to_owned(),
                external_id: "01HX".to_owned(),
                title: "Weekly sync".to_owned(),
                started_at: Utc::now(),
                duration_ms: None,
                organizer: String::new(),
                participants: json!([]),
                url: None,
                transcript: json!([]),
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("insert")
        .expect("new");

        // The loop starts the meeting's run, and its fetch fails for good.
        let started = workflows::start(&mut connection, &events, run_for(pending.id))
            .await
            .expect("the run starts");
        let at = Utc::now();
        let claims = step_job::claim(&mut connection, at, 1)
            .await
            .expect("claims")
            .claims;
        step_job::finish(
            &mut connection,
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

        // Its next pass is answered with the same failed run, and fails the meeting with it.
        let followed = workflows::start(&mut connection, &events, run_for(pending.id))
            .await
            .expect("answers");
        assert!(!followed.created);
        assert_eq!(followed.settled.run.id, started.settled.run.id);
        assert_eq!(followed.settled.run.state, WorkflowRunState::Failed);
        let reason = followed.settled.run.error.expect("a failed run says why");
        meeting::record_fetch_failure(&mut connection, pending.id, &reason)
            .await
            .expect("records")
            .expect("a pending meeting fails");

        // Retrying the meeting queues its run's fetch again, as the person who asked.
        let retried = retry(&mut connection, &events, pending.id, TEST_PERSON_ID)
            .await
            .expect("retried");
        assert_eq!(retried.state(), MeetingState::Pending);
        let run = workflow_run::find(&mut connection, started.settled.run.id)
            .await
            .expect("the run");
        assert_ne!(run.state, WorkflowRunState::Failed);
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

        // So the next pass follows the run going again, and leaves the meeting waiting on it.
        let next_pass = workflows::start(&mut connection, &events, run_for(pending.id))
            .await
            .expect("answers");
        assert_eq!(next_pass.settled.run.id, run.id);
        assert_ne!(next_pass.settled.run.state, WorkflowRunState::Failed);
    }
}
