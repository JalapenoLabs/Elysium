// Copyright © 2026 Jalapeno Labs

//! The queue against Postgres (#75): runs worked end to end by the step workers, claims that
//! never overlap, leases that lapse and are taken back, attempts and their backoff, a person's
//! retry, skip, and resume, and the workers giving steps back at shutdown.
//!
//! Instants are passed in rather than read from the clock, so a lease lapsing or a backoff
//! passing is a later `at`, not a wait.

use std::time::Duration;

use chrono::{TimeDelta, Utc};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{new_run, person, pool, running_workers, start, test_steps, until_run_is};
use crate::models::step_job::{
    self, LEASE, MAX_ATTEMPTS, StepActionError, StepJobState, StepOutcome,
};
use crate::models::workflow_run::{self, WorkflowRunState};
use crate::realtime::EventBus;
use crate::test_support::{TEST_PERSON_ID, migrated_database};

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_workers_run_a_workflow_step_by_step_each_receiving_the_last_output() {
    let (url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let mut heard = events.subscribe();
    let (shutdown, running) = running_workers(&url, &events, test_steps().0).await;

    let started = crate::workflows::start(
        &mut connection,
        &events,
        new_run("m-1", &["double", "describe"]),
    )
    .await
    .expect("the run starts");
    assert!(started.created);
    let run = until_run_is(
        &mut connection,
        started.settled.run.id,
        WorkflowRunState::Succeeded,
    )
    .await;

    assert!(run.started_at.is_some() && run.finished_at.is_some());
    let steps = workflow_run::steps_of(&mut connection, run.id)
        .await
        .expect("steps read");
    assert_eq!(steps[0].output, Some(json!({ "n": 42 })));
    assert_eq!(steps[0].summary.as_deref(), Some("Doubled 21"));
    assert_eq!(
        steps[1].input,
        Some(json!({ "n": 42 })),
        "fed the step before's output"
    );
    assert_eq!(
        steps[1].output,
        Some(json!({
            "input": { "n": 42 },
            "runId": run.id,
            "stepJobId": steps[1].id,
            "attempt": 1,
        })),
        "a step is called with its run, its own row, and which attempt it is"
    );
    let log = step_job::log_of(&mut connection, run.id, steps[0].id)
        .await
        .expect("log reads");
    assert_eq!(log, "doubling 21\n");

    shutdown.cancel();
    running.await.expect("the workers stop");
    let mut kinds = Vec::new();
    while let Ok(payload) = heard.try_recv() {
        let event: Value = serde_json::from_str(&payload).expect("events are JSON");
        assert!(
            event["data"].get("input").is_none() && event["data"].get("log").is_none(),
            "events carry ids and states alone: {event}"
        );
        kinds.push(format!(
            "{} {}",
            event["type"].as_str().unwrap_or_default(),
            event["data"]["state"]
        ));
    }
    assert!(
        kinds.contains(&"workflowRun.changed \"queued\"".to_owned()),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&"stepJob.changed \"running\"".to_owned()),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&"workflowRun.changed \"succeeded\"".to_owned()),
        "{kinds:?}"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn starting_a_workflow_on_a_source_again_answers_the_live_run() {
    let (_url, mut connection) = migrated_database().await;
    let first = start(&mut connection, "m-1", &["double"]).await;
    let again = start(&mut connection, "m-1", &["double"]).await;
    assert!(!again.created);
    assert_eq!(again.settled.run.id, first.settled.run.id);

    let at = Utc::now();
    let claimed = step_job::claim(&mut connection, at, 10)
        .await
        .expect("claims");
    step_job::finish(
        &mut connection,
        &claimed.claims[0],
        StepOutcome::Succeeded {
            output: json!({}),
            summary: None,
        },
        at,
    )
    .await
    .expect("records");
    let after_success = start(&mut connection, "m-1", &["double"]).await;
    assert!(
        after_success.created,
        "a succeeded run leaves the source free"
    );
    assert_ne!(after_success.settled.run.id, first.settled.run.id);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn two_claimers_never_take_the_same_step() {
    let (url, mut connection) = migrated_database().await;
    for source in 0..12 {
        start(&mut connection, &format!("s-{source}"), &["double"]).await;
    }
    let database = pool(&url).await;
    let mut first = database.get().await.expect("a connection");
    let mut second = database.get().await.expect("a connection");
    let at = Utc::now();

    let (one, two) = tokio::join!(
        step_job::claim(&mut first, at, 8),
        step_job::claim(&mut second, at, 8),
    );
    let mut ids: Vec<Uuid> = one
        .expect("claims")
        .claims
        .into_iter()
        .chain(two.expect("claims").claims)
        .map(|claim| claim.job.id)
        .collect();
    assert_eq!(ids.len(), 12, "every due step is claimed once between them");
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 12, "no step is claimed twice");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_lapsed_lease_is_taken_back_and_the_old_claim_records_nothing() {
    let (_url, mut connection) = migrated_database().await;
    let run = start(&mut connection, "m-1", &["double"]).await.settled.run;
    let at = Utc::now();
    let first = step_job::claim(&mut connection, at, 1)
        .await
        .expect("claims")
        .claims;
    assert_eq!(first.len(), 1);
    assert!(
        step_job::claim(&mut connection, at + LEASE - TimeDelta::seconds(1), 1)
            .await
            .expect("claims")
            .claims
            .is_empty(),
        "a held lease is left alone"
    );

    let lapsed = at + LEASE + TimeDelta::seconds(1);
    let second = step_job::claim(&mut connection, lapsed, 1)
        .await
        .expect("claims")
        .claims;
    assert_eq!(second.len(), 1, "the dead engine's step is taken back");
    assert_eq!(second[0].job.attempts, 2, "the lost attempt counts");
    assert_ne!(second[0].token, first[0].token);

    assert!(
        !step_job::renew(&mut connection, &first[0], lapsed)
            .await
            .expect("renews")
    );
    assert!(
        !step_job::append_log(&mut connection, &first[0], "late")
            .await
            .expect("appends")
    );
    let stale = step_job::finish(
        &mut connection,
        &first[0],
        StepOutcome::Succeeded {
            output: json!({ "stale": true }),
            summary: None,
        },
        lapsed,
    )
    .await
    .expect("records");
    assert!(
        stale.is_none(),
        "an attempt whose claim was lost records nothing"
    );

    let settled = step_job::finish(
        &mut connection,
        &second[0],
        StepOutcome::Succeeded {
            output: json!({ "n": 42 }),
            summary: None,
        },
        lapsed,
    )
    .await
    .expect("records")
    .expect("the live claim records");
    assert_eq!(settled.run.id, run.id);
    assert_eq!(settled.run.state, WorkflowRunState::Succeeded);
    assert_eq!(settled.steps[0].output, Some(json!({ "n": 42 })));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_retryable_failure_backs_off_until_its_attempts_run_out_and_stops_the_run() {
    let (_url, mut connection) = migrated_database().await;
    let run_id = start(&mut connection, "m-1", &["double", "describe"])
        .await
        .settled
        .run
        .id;
    let mut at = Utc::now();
    for attempt in 1..=MAX_ATTEMPTS {
        let claims = step_job::claim(&mut connection, at, 1)
            .await
            .expect("claims")
            .claims;
        assert_eq!(claims.len(), 1, "attempt {attempt} is due");
        assert_eq!(claims[0].job.attempts, attempt);
        let settled = step_job::finish(
            &mut connection,
            &claims[0],
            StepOutcome::Failed {
                reason: "the provider timed out".to_owned(),
                retryable: true,
            },
            at,
        )
        .await
        .expect("records")
        .expect("the claim holds");
        if attempt < MAX_ATTEMPTS {
            assert_eq!(settled.steps[0].state, StepJobState::Queued);
            let due = settled.steps[0]
                .available_at
                .expect("queued with a due time");
            assert!(due > at, "it backs off");
            assert!(
                step_job::claim(&mut connection, due - TimeDelta::seconds(1), 1)
                    .await
                    .expect("claims")
                    .claims
                    .is_empty(),
                "not before its backoff passes"
            );
            assert_eq!(settled.run.state, WorkflowRunState::Running);
            at = due;
        } else {
            assert_eq!(settled.steps[0].state, StepJobState::Failed);
            assert_eq!(
                settled.steps[1].state,
                StepJobState::Pending,
                "the run stops at it"
            );
            assert_eq!(settled.run.state, WorkflowRunState::Failed);
            assert_eq!(settled.run.error.as_deref(), Some("the provider timed out"));
            assert!(settled.run.finished_at.is_some());
        }
    }
    let steps = workflow_run::steps_of(&mut connection, run_id)
        .await
        .expect("steps read");
    let log = step_job::log_of(&mut connection, run_id, steps[0].id)
        .await
        .expect("log reads");
    assert_eq!(
        log.lines().count(),
        3,
        "every failed attempt is in the log: {log}"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_failure_that_may_not_be_retried_fails_the_step_at_once() {
    let (_url, mut connection) = migrated_database().await;
    start(&mut connection, "m-1", &["double"]).await;
    let at = Utc::now();
    let claims = step_job::claim(&mut connection, at, 1)
        .await
        .expect("claims")
        .claims;
    let settled = step_job::finish(
        &mut connection,
        &claims[0],
        StepOutcome::Failed {
            reason: "the meeting has no transcript".to_owned(),
            retryable: false,
        },
        at,
    )
    .await
    .expect("records")
    .expect("the claim holds");
    assert_eq!(settled.steps[0].state, StepJobState::Failed);
    assert_eq!(settled.run.state, WorkflowRunState::Failed);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_step_whose_engine_died_on_every_attempt_is_failed_with_the_reason() {
    let (_url, mut connection) = migrated_database().await;
    start(&mut connection, "m-1", &["double"]).await;
    let mut at = Utc::now();
    for _ in 0..MAX_ATTEMPTS {
        let claims = step_job::claim(&mut connection, at, 1)
            .await
            .expect("claims")
            .claims;
        assert_eq!(claims.len(), 1);
        // Its engine dies: nothing renews the lease.
        at += LEASE + TimeDelta::seconds(1);
    }
    assert!(
        step_job::claim(&mut connection, at, 1)
            .await
            .expect("claims")
            .claims
            .is_empty(),
        "no attempts are left to claim"
    );

    let reaped = step_job::reap(&mut connection, at).await.expect("reaps");
    assert_eq!(reaped.len(), 1);
    assert_eq!(reaped[0].steps[0].state, StepJobState::Failed);
    assert_eq!(reaped[0].run.state, WorkflowRunState::Failed);
    assert!(
        reaped[0]
            .run
            .error
            .as_deref()
            .is_some_and(|error| error.contains("stopped before it finished")),
        "{:?}",
        reaped[0].run.error
    );
    assert!(
        step_job::reap(&mut connection, at)
            .await
            .expect("reaps")
            .is_empty()
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_person_retries_a_failed_step_and_the_run_goes_on() {
    let (_url, mut connection) = migrated_database().await;
    let run_id = start(&mut connection, "m-1", &["double", "describe"])
        .await
        .settled
        .run
        .id;
    let at = Utc::now();
    let claims = step_job::claim(&mut connection, at, 1)
        .await
        .expect("claims")
        .claims;
    let step_id = claims[0].job.id;
    let refused = step_job::retry(&mut connection, run_id, step_id, person(at)).await;
    assert!(
        matches!(refused, Err(StepActionError::Refused(_))),
        "a running step is not retried"
    );
    step_job::finish(
        &mut connection,
        &claims[0],
        StepOutcome::Failed {
            reason: "no transcript yet".to_owned(),
            retryable: false,
        },
        at,
    )
    .await
    .expect("records");

    let retried = step_job::retry(&mut connection, run_id, step_id, person(at))
        .await
        .expect("a failed step is retried");
    assert_eq!(retried.steps[0].state, StepJobState::Queued);
    assert_eq!(retried.steps[0].attempts, 0, "with a fresh set of attempts");
    assert_eq!(retried.run.state, WorkflowRunState::Running);
    assert_eq!(retried.run.error, None);
    assert_eq!(retried.run.finished_at, None);
    assert_eq!(retried.moved, [step_id]);

    let history = workflow_run::history(&mut connection, run_id)
        .await
        .expect("history reads");
    let kinds: Vec<&str> = history.iter().map(|entry| entry.kind.as_str()).collect();
    assert_eq!(kinds, ["started", "retried"]);
    assert_eq!(history[1].created_by, TEST_PERSON_ID);
    assert_eq!(history[1].step_job_id, Some(step_id));
    assert_eq!(history[1].data, json!({ "error": "no transcript yet" }));

    let claims = step_job::claim(&mut connection, at, 1)
        .await
        .expect("claims")
        .claims;
    assert_eq!(claims[0].job.id, step_id, "it is claimed again at once");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_skipped_step_passes_its_input_to_the_next() {
    let (_url, mut connection) = migrated_database().await;
    let started = start(&mut connection, "m-1", &["double", "describe", "double"]).await;
    let run_id = started.settled.run.id;
    let first = started.settled.steps[0].id;
    let at = Utc::now();

    let skipped = step_job::skip(&mut connection, run_id, first, person(at))
        .await
        .expect("a queued step is skipped");
    assert_eq!(skipped.steps[0].state, StepJobState::Skipped);
    assert_eq!(skipped.steps[1].state, StepJobState::Queued);
    assert_eq!(
        skipped.steps[1].input,
        Some(json!({ "n": 21 })),
        "the run's input, passed on"
    );
    assert_eq!(skipped.moved, [first, skipped.steps[1].id]);

    // A pending step skipped ahead of time is passed over when the run reaches it.
    let last = skipped.steps[2].id;
    step_job::skip(&mut connection, run_id, last, person(at))
        .await
        .expect("skipped ahead");
    let claims = step_job::claim(&mut connection, at, 1)
        .await
        .expect("claims")
        .claims;
    let settled = step_job::finish(
        &mut connection,
        &claims[0],
        StepOutcome::Succeeded {
            output: json!({ "description": "done" }),
            summary: None,
        },
        at,
    )
    .await
    .expect("records")
    .expect("the claim holds");
    assert_eq!(settled.run.state, WorkflowRunState::Succeeded);

    let refused = step_job::skip(&mut connection, run_id, first, person(at)).await;
    assert!(
        matches!(refused, Err(StepActionError::Refused(_))),
        "a step is skipped once"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_held_step_waits_for_a_person_and_resuming_passes_its_output_on() {
    let (_url, mut connection) = migrated_database().await;
    let run_id = start(&mut connection, "m-1", &["double", "describe"])
        .await
        .settled
        .run
        .id;
    let at = Utc::now();
    let claims = step_job::claim(&mut connection, at, 1)
        .await
        .expect("claims")
        .claims;
    let step_id = claims[0].job.id;
    let held = step_job::finish(
        &mut connection,
        &claims[0],
        StepOutcome::Held {
            output: json!({ "n": 42 }),
            summary: "Review the corrected transcript".to_owned(),
        },
        at,
    )
    .await
    .expect("records")
    .expect("the claim holds");
    assert_eq!(held.run.state, WorkflowRunState::Held);
    assert_eq!(
        held.steps[1].state,
        StepJobState::Pending,
        "nothing goes on while it is held"
    );
    assert!(
        step_job::claim(&mut connection, at, 1)
            .await
            .expect("claims")
            .claims
            .is_empty(),
        "nothing is due"
    );

    let resumed = step_job::resume(&mut connection, run_id, step_id, person(at))
        .await
        .expect("a held step is resumed");
    assert_eq!(resumed.steps[0].state, StepJobState::Succeeded);
    assert_eq!(resumed.steps[1].state, StepJobState::Queued);
    assert_eq!(
        resumed.steps[1].input,
        Some(json!({ "n": 42 })),
        "the held output passed on"
    );
    assert_eq!(resumed.run.state, WorkflowRunState::Running);

    let refused = step_job::resume(&mut connection, run_id, step_id, person(at)).await;
    assert!(
        matches!(refused, Err(StepActionError::Refused(_))),
        "only a held step resumes"
    );
    let missing = step_job::resume(&mut connection, Uuid::now_v7(), step_id, person(at)).await;
    assert!(
        matches!(
            missing,
            Err(StepActionError::Database(diesel::result::Error::NotFound))
        ),
        "a step is acted on through its own run"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_released_step_is_queued_again_without_counting_the_attempt() {
    let (_url, mut connection) = migrated_database().await;
    start(&mut connection, "m-1", &["double"]).await;
    let at = Utc::now();
    let claims = step_job::claim(&mut connection, at, 1)
        .await
        .expect("claims")
        .claims;
    assert!(
        step_job::release(&mut connection, &claims[0], at)
            .await
            .expect("releases")
    );
    assert!(
        !step_job::release(&mut connection, &claims[0], at)
            .await
            .expect("releases"),
        "once"
    );

    let again = step_job::claim(&mut connection, at, 1)
        .await
        .expect("claims")
        .claims;
    assert_eq!(again.len(), 1, "it is due at once");
    assert_eq!(
        again[0].job.attempts, 1,
        "the released attempt did not count"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_workers_fail_a_step_this_build_does_not_have_and_release_one_at_shutdown() {
    let (url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let unknown = start(&mut connection, "m-1", &["transcribe"])
        .await
        .settled
        .run
        .id;
    let forever = start(&mut connection, "m-2", &["wait_forever"])
        .await
        .settled
        .run
        .id;
    let (shutdown, running) = running_workers(&url, &events, test_steps().0).await;

    let failed = until_run_is(&mut connection, unknown, WorkflowRunState::Failed).await;
    assert_eq!(
        failed.error.as_deref(),
        Some("This build of Elysium has no step named transcribe.")
    );
    until_run_is(&mut connection, forever, WorkflowRunState::Running).await;

    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .expect("the workers stop promptly")
        .expect("the workers stop cleanly");
    let steps = workflow_run::steps_of(&mut connection, forever)
        .await
        .expect("steps read");
    assert_eq!(
        steps[0].state,
        StepJobState::Queued,
        "given back for the next engine"
    );
    assert_eq!(steps[0].attempts, 0);
}
