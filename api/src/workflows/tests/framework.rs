// Copyright © 2026 Jalapeno Labs

//! The step and workflow framework (#76): workflows composed from the same steps in different
//! orders and subsets, a run entering a workflow at a later step, a person retrying one step
//! while the rest stand, the queue's JSON read back as each step's own types, and the model
//! setting a step's turn reads.

use std::sync::atomic::Ordering;

use chrono::Utc;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use serde_json::json;
use uuid::Uuid;

use super::{
    ChooseModel, Count, Describe, Double, FailOnce, Increment, NothingToDo, Number, Report, Review,
    person, running_workers, test_steps, until_run_is,
};
use crate::agent_models::{self, AgentKind};
use crate::database::Pool;
use crate::models::model_setting::{self, Effort, ModelChoice};
use crate::models::step_job::{self, StepOutcome};
use crate::models::user::SYSTEM_USER_ID;
use crate::models::workflow_run::{self, WorkflowRunState};
use crate::realtime::{EventBus, Signal};
use crate::test_support::{TEST_PERSON_ID, migrated_database};
use crate::workflows::{self, Start, StepContext, Steps, Workflow, declare};

/// Doubles, adds one, then says what it ended with.
fn double_then_increment() -> Workflow<Number> {
    declare("double_then_increment")
        .first::<Double>()
        .then::<Increment>()
        .then::<Describe>()
        .build()
}

/// The same steps, in another order and without the last.
fn increment_then_double() -> Workflow<Number> {
    declare("increment_then_double")
        .first::<Increment>()
        .then::<Double>()
        .build()
}

/// One of the same steps alone.
fn describe_only() -> Workflow<Number> {
    declare("describe_only").first::<Describe>().build()
}

/// A step that fails once, between two that do not.
fn flaky() -> Workflow<Number> {
    declare("flaky")
        .first::<Double>()
        .then::<FailOnce>()
        .then::<Describe>()
        .build()
}

/// A step whose output is a type other than its input, between two others.
fn tallying() -> Workflow<Number> {
    declare("tallying")
        .first::<Double>()
        .then::<Count>()
        .then::<Report>()
        .build()
}

fn started_with<Input>(source_id: &str, input: Input) -> Start<Input> {
    Start {
        source_kind: "test_source".to_owned(),
        source_id: source_id.to_owned(),
        input,
        created_by: SYSTEM_USER_ID,
    }
}

/// A pool that never connects, for calling a step whose attempt never reads the database.
fn unconnected_pool() -> Pool {
    let manager = AsyncDieselConnectionManager::new("postgres://unused.invalid/none");
    Pool::builder(manager)
        .build()
        .expect("a pool builds without connecting")
}

/// What one step of a run gave, by its position in the run.
async fn output_of(
    connection: &mut diesel_async::AsyncPgConnection,
    run_id: Uuid,
    position: usize,
) -> serde_json::Value {
    let steps = workflow_run::steps_of(connection, run_id)
        .await
        .expect("steps read");
    steps[position]
        .output
        .clone()
        .expect("the step gave output")
}

#[test]
fn a_workflow_runs_its_steps_from_the_first_or_from_the_step_it_is_entered_at() {
    let workflow = double_then_increment();
    let from_the_start = workflow.new_run(started_with("m-1", Number { n: 21 }));
    assert_eq!(from_the_start.workflow, "double_then_increment");
    assert_eq!(from_the_start.steps, ["double", "increment", "describe"]);
    assert_eq!(from_the_start.input, json!({ "n": 21 }));

    let entered = workflow.new_run_at::<Increment>(started_with("m-2", Number { n: 5 }));
    assert_eq!(
        entered.steps,
        ["increment", "describe"],
        "the steps before it are not part of the run"
    );
    assert_eq!(entered.input, json!({ "n": 5 }));
    assert_eq!(entered.source_id, "m-2");
}

#[test]
#[should_panic(expected = "workflow increment_then_double has no step describe")]
fn entering_a_workflow_at_a_step_it_does_not_declare_is_a_programming_error() {
    increment_then_double().new_run_at::<Describe>(started_with("m-1", Number { n: 1 }));
}

#[test]
fn every_test_workflow_names_only_steps_that_are_registered() {
    let (steps, _calls) = test_steps();
    steps.assert_runs(&double_then_increment().outline());
    steps.assert_runs(&increment_then_double().outline());
    steps.assert_runs(&describe_only().outline());
    steps.assert_runs(&flaky().outline());
    steps.assert_runs(&tallying().outline());
}

#[test]
#[should_panic(expected = "workflow tallying names step count, which is not registered")]
fn a_workflow_naming_a_step_nobody_registered_is_caught_before_it_runs() {
    let mut steps = Steps::new();
    steps.register(Double::default());
    steps.register(Report);
    steps.assert_runs(&tallying().outline());
}

/// A step that borrows another's name, as a typo or a copied step would.
#[derive(Debug)]
struct Impostor;

impl workflows::Step for Impostor {
    const NAME: &'static str = "double";
    const AGENT_KIND: Option<AgentKind> = None;
    type Input = Number;
    type Output = Number;

    async fn run(
        &self,
        input: Number,
        _context: StepContext,
    ) -> Result<workflows::Done<Number>, workflows::StepFailure> {
        Ok(workflows::Done::Succeeded {
            output: input,
            summary: None,
        })
    }
}

#[test]
#[should_panic(expected = "as a type other than the one registered by that name")]
fn a_workflow_naming_another_type_by_a_registered_name_is_caught() {
    let (steps, _calls) = test_steps();
    let workflow = declare("impostor").first::<Impostor>().build();
    steps.assert_runs(&workflow.outline());
}

#[test]
fn every_registered_step_a_declared_workflow_names_passes() {
    let mut steps = Steps::new();
    steps.register(Double::default());
    steps.register(Increment);
    steps.assert_declared_in(&[double_then_increment().outline()]);
}

#[test]
#[should_panic(
    expected = "step report is registered, but no workflow in workflows::declared names it"
)]
fn a_registered_step_no_declared_workflow_names_is_caught_so_the_settings_page_lists_it() {
    let mut steps = Steps::new();
    steps.register(Double::default());
    steps.register(Increment);
    steps.register(Report);
    steps.assert_declared_in(&[double_then_increment().outline()]);
}

#[tokio::test]
#[should_panic(expected = "step double reads its model setting but declares AGENT_KIND = None")]
async fn a_step_that_runs_no_model_reading_a_model_setting_is_a_programming_error() {
    let context = StepContext::for_test::<Double>(unconnected_pool());
    drop(context.model().await);
}

#[test]
#[should_panic(expected = "step double is registered twice")]
fn a_name_is_registered_once() {
    let (mut steps, _calls) = test_steps();
    steps.register(Impostor);
}

#[test]
#[should_panic(expected = "workflow Meeting-Notes is not named in snake_case")]
fn a_workflow_is_named_in_snake_case() {
    declare("Meeting-Notes");
}

#[test]
fn names_are_snake_case_from_a_letter_and_fit_the_queue() {
    for name in ["double", "fetch_transcript", "step2", &"a".repeat(64)] {
        assert!(workflows::is_name(name), "{name}");
    }
    for name in [
        "",
        "2fast",
        "_hidden",
        "Double",
        "fetch-transcript",
        &"a".repeat(65),
    ] {
        assert!(!workflows::is_name(name), "{name}");
    }
}

#[tokio::test]
async fn json_a_step_cannot_read_as_its_input_fails_it_for_good_naming_it() {
    let (steps, _calls) = test_steps();
    let report = steps.find("report").expect("registered").run;
    let context = StepContext::for_test::<Report>(unconnected_pool());

    let outcome = report(json!({ "n": 42 }), context).await;
    let StepOutcome::Failed { reason, retryable } = outcome else {
        panic!("a step never runs on input it cannot read: {outcome:?}");
    };
    assert!(!retryable, "every attempt would read the same JSON");
    assert!(
        reason.starts_with("The input report received is not what it takes (missing field `total`"),
        "{reason}"
    );
    assert!(
        reason.contains("skipped"),
        "it names the likely cause: {reason}"
    );
}

#[tokio::test]
async fn a_step_answering_in_its_own_types_is_stored_as_json() {
    let (steps, _calls) = test_steps();
    let count = steps.find("count").expect("registered").run;
    let context = StepContext::for_test::<Count>(unconnected_pool());

    assert_eq!(
        count(json!({ "n": 7 }), context).await,
        StepOutcome::Succeeded {
            output: json!({ "total": 7 }),
            summary: None,
        }
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn workflows_compose_the_same_steps_in_any_order_and_subset() {
    let (url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let (shutdown, running) = running_workers(&url, &events, test_steps().0).await;

    let mut runs = Vec::new();
    for new in [
        double_then_increment().new_run(started_with("m-1", Number { n: 21 })),
        increment_then_double().new_run(started_with("m-1", Number { n: 21 })),
        describe_only().new_run(started_with("m-1", Number { n: 21 })),
    ] {
        let started = workflows::start(&mut connection, &events, new)
            .await
            .expect("the run starts");
        runs.push(started.settled.run.id);
    }
    for run_id in &runs {
        until_run_is(&mut connection, *run_id, WorkflowRunState::Succeeded).await;
    }

    assert_eq!(
        output_of(&mut connection, runs[0], 2).await["input"],
        json!({ "n": 43 }),
        "doubled, then one added"
    );
    assert_eq!(
        output_of(&mut connection, runs[1], 1).await,
        json!({ "n": 44 }),
        "one added, then doubled"
    );
    assert_eq!(
        output_of(&mut connection, runs[2], 0).await["input"],
        json!({ "n": 21 }),
        "described alone"
    );

    shutdown.cancel();
    running.await.expect("the workers stop");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_run_entered_at_a_later_step_runs_from_there_with_that_steps_input() {
    let (url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let (steps, calls) = test_steps();
    let (shutdown, running) = running_workers(&url, &events, steps).await;

    let new = double_then_increment().new_run_at::<Increment>(started_with("m-1", Number { n: 5 }));
    let started = workflows::start(&mut connection, &events, new)
        .await
        .expect("the run starts");
    let run = until_run_is(
        &mut connection,
        started.settled.run.id,
        WorkflowRunState::Succeeded,
    )
    .await;

    let steps = workflow_run::steps_of(&mut connection, run.id)
        .await
        .expect("steps read");
    let names: Vec<&str> = steps.iter().map(|step| step.step.as_str()).collect();
    assert_eq!(names, ["increment", "describe"]);
    assert_eq!(steps[0].input, Some(json!({ "n": 5 })));
    assert_eq!(steps[1].input, Some(json!({ "n": 6 })));
    assert_eq!(
        calls.double.load(Ordering::SeqCst),
        0,
        "the step before the entry never runs"
    );
    let history = workflow_run::history(&mut connection, run.id)
        .await
        .expect("history reads");
    assert_eq!(
        history[0].data,
        json!({ "steps": ["increment", "describe"] })
    );

    shutdown.cancel();
    running.await.expect("the workers stop");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_person_retries_the_one_step_that_failed_and_the_run_goes_on_from_it() {
    let (url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let (steps, calls) = test_steps();
    let (shutdown, running) = running_workers(&url, &events, steps).await;

    let started = workflows::start(
        &mut connection,
        &events,
        flaky().new_run(started_with("m-1", Number { n: 21 })),
    )
    .await
    .expect("the run starts");
    let run_id = started.settled.run.id;
    let failed = until_run_is(&mut connection, run_id, WorkflowRunState::Failed).await;
    assert_eq!(
        failed.error.as_deref(),
        Some("the transcript is not ready yet")
    );
    let steps = workflow_run::steps_of(&mut connection, run_id)
        .await
        .expect("steps read");
    assert_eq!(steps[2].state, step_job::StepJobState::Pending);

    step_job::retry(&mut connection, run_id, steps[1].id, person(Utc::now()))
        .await
        .expect("a failed step is retried");
    events.signal(&Signal::Workflows);
    until_run_is(&mut connection, run_id, WorkflowRunState::Succeeded).await;

    assert_eq!(
        calls.double.load(Ordering::SeqCst),
        1,
        "the step before it stands as it was"
    );
    assert_eq!(calls.fail_once.load(Ordering::SeqCst), 2);
    let steps = workflow_run::steps_of(&mut connection, run_id)
        .await
        .expect("steps read");
    assert_eq!(steps[1].attempts, 1, "a fresh set of attempts");
    assert_eq!(
        steps[2].input,
        Some(json!({ "n": 42 })),
        "the step after it takes the retried step's output"
    );

    shutdown.cancel();
    running.await.expect("the workers stop");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_held_step_stops_its_run_until_resumed_and_then_passes_its_output_on() {
    let (url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let (shutdown, running) = running_workers(&url, &events, test_steps().0).await;
    let reviewed = declare("reviewed")
        .first::<Double>()
        .then::<Review>()
        .then::<Describe>()
        .build();

    let run_id = workflows::start(
        &mut connection,
        &events,
        reviewed.new_run(started_with("m-1", Number { n: 21 })),
    )
    .await
    .expect("the run starts")
    .settled
    .run
    .id;
    until_run_is(&mut connection, run_id, WorkflowRunState::Held).await;
    let steps = workflow_run::steps_of(&mut connection, run_id)
        .await
        .expect("steps read");
    assert_eq!(
        steps[1].summary.as_deref(),
        Some("Check 42 before it goes on")
    );

    step_job::resume(&mut connection, run_id, steps[1].id, person(Utc::now()))
        .await
        .expect("a held step is resumed");
    events.signal(&Signal::Workflows);
    until_run_is(&mut connection, run_id, WorkflowRunState::Succeeded).await;
    assert_eq!(
        output_of(&mut connection, run_id, 2).await["input"],
        json!({ "n": 42 })
    );

    shutdown.cancel();
    running.await.expect("the workers stop");
}

#[tokio::test]
async fn a_step_with_nothing_to_do_skips_itself_saying_why() {
    let (steps, _calls) = test_steps();
    let nothing_to_do = steps.find("nothing_to_do").expect("registered").run;
    let context = StepContext::for_test::<NothingToDo>(unconnected_pool());

    assert_eq!(
        nothing_to_do(json!({ "n": 7 }), context).await,
        StepOutcome::Skipped {
            reason: "Nothing to do with 7".to_owned(),
        }
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_step_that_skips_itself_hands_the_next_its_input_and_the_run_goes_on() {
    let (url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let (shutdown, running) = running_workers(&url, &events, test_steps().0).await;
    let optional = declare("optional")
        .first::<Double>()
        .then::<NothingToDo>()
        .then::<Describe>()
        .build();

    let run_id = workflows::start(
        &mut connection,
        &events,
        optional.new_run(started_with("m-1", Number { n: 21 })),
    )
    .await
    .expect("the run starts")
    .settled
    .run
    .id;
    until_run_is(&mut connection, run_id, WorkflowRunState::Succeeded).await;
    let steps = workflow_run::steps_of(&mut connection, run_id)
        .await
        .expect("steps read");
    assert_eq!(steps[1].state, step_job::StepJobState::Skipped);
    assert_eq!(steps[1].summary.as_deref(), Some("Nothing to do with 42"));
    assert_eq!(steps[1].output, None, "a skipped step gives nothing");
    assert_eq!(
        output_of(&mut connection, run_id, 2).await["input"],
        json!({ "n": 42 }),
        "the step after it received its input"
    );
    // Nobody skipped it, so the run's history records no person's skip.
    let history = workflow_run::history(&mut connection, run_id)
        .await
        .expect("history");
    assert!(history.iter().all(|entry| entry.kind != "skipped"));

    shutdown.cancel();
    running.await.expect("the workers stop");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn skipping_a_step_the_next_needs_fails_the_next_with_the_reason() {
    let (url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let started = workflows::start(
        &mut connection,
        &events,
        tallying().new_run(started_with("m-1", Number { n: 21 })),
    )
    .await
    .expect("the run starts");
    let run_id = started.settled.run.id;
    // Skipped ahead, before the workers take the run.
    let count = started.settled.steps[1].id;
    step_job::skip(&mut connection, run_id, count, person(Utc::now()))
        .await
        .expect("a pending step is skipped");

    let (shutdown, running) = running_workers(&url, &events, test_steps().0).await;
    let failed = until_run_is(&mut connection, run_id, WorkflowRunState::Failed).await;
    let reason = failed.error.expect("a failed run says why");
    assert!(
        reason.starts_with("The input report received is not what it takes"),
        "{reason}"
    );
    let steps = workflow_run::steps_of(&mut connection, run_id)
        .await
        .expect("steps read");
    assert_eq!(
        steps[2].input,
        Some(json!({ "n": 42 })),
        "it received a number"
    );

    shutdown.cancel();
    running.await.expect("the workers stop");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_steps_turn_runs_on_its_setting_read_as_it_starts_and_opus_at_high_otherwise() {
    let (url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let (shutdown, running) = running_workers(&url, &events, test_steps().0).await;
    let workflow = declare("choosing").first::<ChooseModel>().build();

    let unset = workflows::start(
        &mut connection,
        &events,
        workflow.new_run(started_with("m-1", Number { n: 1 })),
    )
    .await
    .expect("the run starts")
    .settled
    .run
    .id;
    until_run_is(&mut connection, unset, WorkflowRunState::Succeeded).await;
    assert_eq!(
        output_of(&mut connection, unset, 0).await,
        json!({ "model": "claude-opus-5-5", "effort": "high" })
    );
    let steps = workflow_run::steps_of(&mut connection, unset)
        .await
        .expect("steps read");
    let log = step_job::log_of(&mut connection, unset, steps[0].id)
        .await
        .expect("log reads");
    assert_eq!(
        log,
        "Runs on claude-opus-5-5 at high effort, from its kind's default\n"
    );

    let chosen = ModelChoice {
        model: "claude-sonnet-5-5".to_owned(),
        effort: Effort::Low,
    };
    model_setting::set(&mut connection, "choose_model", &chosen, TEST_PERSON_ID)
        .await
        .expect("the setting is stored");
    let set = workflows::start(
        &mut connection,
        &events,
        workflow.new_run(started_with("m-2", Number { n: 1 })),
    )
    .await
    .expect("the run starts")
    .settled
    .run
    .id;
    until_run_is(&mut connection, set, WorkflowRunState::Succeeded).await;
    assert_eq!(
        output_of(&mut connection, set, 0).await,
        json!({ "model": "claude-sonnet-5-5", "effort": "low" })
    );

    shutdown.cancel();
    running.await.expect("the workers stop");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_step_setting_is_changed_in_place_and_others_keep_the_default() {
    let (_url, mut connection) = migrated_database().await;
    let first = ModelChoice {
        model: "claude-sonnet-5-5".to_owned(),
        effort: Effort::Medium,
    };
    let second = ModelChoice {
        model: "claude-opus-5-5".to_owned(),
        effort: Effort::Max,
    };
    for choice in [&first, &second] {
        model_setting::set(
            &mut connection,
            "correct_transcript",
            choice,
            TEST_PERSON_ID,
        )
        .await
        .expect("the setting is stored");
    }

    assert_eq!(
        agent_models::for_step(
            &mut connection,
            "correct_transcript",
            AgentKind::Correction,
            None
        )
        .await
        .expect("reads")
        .choice,
        second
    );
    assert_eq!(
        agent_models::for_step(
            &mut connection,
            "segment_transcript",
            AgentKind::Segmenting,
            None
        )
        .await
        .expect("reads")
        .choice,
        ModelChoice::default()
    );
    let flag = ModelChoice {
        model: "--dangerously-skip-permissions".to_owned(),
        effort: Effort::High,
    };
    assert!(
        model_setting::set(&mut connection, "correct_transcript", &flag, TEST_PERSON_ID)
            .await
            .is_err(),
        "a model that reads as a flag is refused"
    );
}
