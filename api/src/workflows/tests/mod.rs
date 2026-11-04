// Copyright © 2026 Jalapeno Labs

//! Tests of workflows: the queue every run is on (#75, [`queue`]) and the step and workflow
//! framework over it (#76, [`framework`]), with the test steps and helpers both share.
//!
//! Tests that need Postgres are `#[ignore]`d so `cargo test` stays hermetic;
//! `api/scripts/verify-migrations.sh` runs them.

#![expect(
    clippy::unused_async_trait_impl,
    reason = "test steps are written as real steps are, though most never wait"
)]

mod framework;
mod queue;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::workers::{Workers, WorkersDeps};
use super::{Done, Step, StepContext, StepFailure, Steps};
use crate::activity::Activities;
use crate::database::Pool;
use crate::models::step_model_setting::ModelChoice;
use crate::models::user::SYSTEM_USER_ID;
use crate::models::workflow_run::{self, Actor, NewRun, Started, WorkflowRunState};
use crate::realtime::EventBus;
use crate::test_support::TEST_PERSON_ID;

/// What most test steps take and give.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Number {
    n: i64,
}

/// Doubles its number.
#[derive(Debug, Default)]
struct Double {
    calls: Arc<AtomicUsize>,
}

impl Step for Double {
    const NAME: &'static str = "double";
    type Input = Number;
    type Output = Number;

    async fn run(&self, input: Number, context: StepContext) -> Result<Done<Number>, StepFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        context.log.line(&format!("doubling {}", input.n));
        Ok(Done::Succeeded {
            output: Number { n: input.n * 2 },
            summary: Some(format!("Doubled {}", input.n)),
        })
    }
}

/// Adds one to its number.
#[derive(Debug)]
struct Increment;

impl Step for Increment {
    const NAME: &'static str = "increment";
    type Input = Number;
    type Output = Number;

    async fn run(&self, input: Number, _context: StepContext) -> Result<Done<Number>, StepFailure> {
        Ok(Done::Succeeded {
            output: Number { n: input.n + 1 },
            summary: None,
        })
    }
}

/// What [`Describe`] gives: what it was called with.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Description {
    input: Number,
    run_id: Uuid,
    step_job_id: Uuid,
    attempt: i32,
}

/// Answers what it was called with.
#[derive(Debug)]
struct Describe;

impl Step for Describe {
    const NAME: &'static str = "describe";
    type Input = Number;
    type Output = Description;

    async fn run(
        &self,
        input: Number,
        context: StepContext,
    ) -> Result<Done<Description>, StepFailure> {
        Ok(Done::Succeeded {
            output: Description {
                input,
                run_id: context.run_id,
                step_job_id: context.step_job_id,
                attempt: context.attempt,
            },
            summary: None,
        })
    }
}

/// Fails for good the first time it runs, then passes its number on.
#[derive(Debug, Default)]
struct FailOnce {
    calls: Arc<AtomicUsize>,
}

impl Step for FailOnce {
    const NAME: &'static str = "fail_once";
    type Input = Number;
    type Output = Number;

    async fn run(&self, input: Number, _context: StepContext) -> Result<Done<Number>, StepFailure> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(StepFailure::permanent("the transcript is not ready yet"));
        }
        Ok(Done::Succeeded {
            output: input,
            summary: None,
        })
    }
}

/// What [`Count`] gives and [`Report`] takes: a type other than [`Number`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Tally {
    total: i64,
}

/// Turns a number into a tally.
#[derive(Debug)]
struct Count;

impl Step for Count {
    const NAME: &'static str = "count";
    type Input = Number;
    type Output = Tally;

    async fn run(&self, input: Number, _context: StepContext) -> Result<Done<Tally>, StepFailure> {
        Ok(Done::Succeeded {
            output: Tally { total: input.n },
            summary: None,
        })
    }
}

/// Passes a tally on.
#[derive(Debug)]
struct Report;

impl Step for Report {
    const NAME: &'static str = "report";
    type Input = Tally;
    type Output = Tally;

    async fn run(&self, input: Tally, _context: StepContext) -> Result<Done<Tally>, StepFailure> {
        Ok(Done::Succeeded {
            output: input,
            summary: None,
        })
    }
}

/// Asks a person to look at its number, passing it on once resumed.
#[derive(Debug)]
struct Review;

impl Step for Review {
    const NAME: &'static str = "review";
    type Input = Number;
    type Output = Number;

    async fn run(&self, input: Number, _context: StepContext) -> Result<Done<Number>, StepFailure> {
        Ok(Done::Held {
            output: input,
            summary: format!("Check {} before it goes on", input.n),
        })
    }
}

/// Answers the model and effort its turn would run on.
#[derive(Debug)]
struct ChooseModel;

impl Step for ChooseModel {
    const NAME: &'static str = "choose_model";
    type Input = Number;
    type Output = ModelChoice;

    async fn run(
        &self,
        _input: Number,
        context: StepContext,
    ) -> Result<Done<ModelChoice>, StepFailure> {
        let choice = context.model().await?;
        Ok(Done::Succeeded {
            output: choice,
            summary: None,
        })
    }
}

/// Never returns.
#[derive(Debug)]
struct WaitForever;

impl Step for WaitForever {
    const NAME: &'static str = "wait_forever";
    type Input = Number;
    type Output = Number;

    async fn run(
        &self,
        _input: Number,
        _context: StepContext,
    ) -> Result<Done<Number>, StepFailure> {
        std::future::pending::<()>().await;
        unreachable!("a pending future never resolves")
    }
}

/// How many times the counting test steps ran.
#[derive(Debug, Clone, Default)]
struct Calls {
    double: Arc<AtomicUsize>,
    fail_once: Arc<AtomicUsize>,
}

/// Every test step, registered, and the counters of those that count their calls.
fn test_steps() -> (Steps, Calls) {
    let calls = Calls::default();
    let mut steps = Steps::new();
    steps.register(Double {
        calls: Arc::clone(&calls.double),
    });
    steps.register(Increment);
    steps.register(Describe);
    steps.register(FailOnce {
        calls: Arc::clone(&calls.fail_once),
    });
    steps.register(Count);
    steps.register(Report);
    steps.register(Review);
    steps.register(ChooseModel);
    steps.register(WaitForever);
    (steps, calls)
}

/// A run of the queue's own test workflow, by step names, as the queue's tests start them.
fn new_run(source_id: &str, steps: &[&str]) -> NewRun {
    NewRun {
        workflow: "test".to_owned(),
        source_kind: "test_source".to_owned(),
        source_id: source_id.to_owned(),
        input: json!({ "n": 21 }),
        steps: steps.iter().map(|step| (*step).to_owned()).collect(),
        created_by: SYSTEM_USER_ID,
    }
}

async fn start(connection: &mut AsyncPgConnection, source_id: &str, steps: &[&str]) -> Started {
    workflow_run::start(connection, new_run(source_id, steps), Utc::now())
        .await
        .expect("the run starts")
}

fn person(at: DateTime<Utc>) -> Actor {
    Actor {
        user_id: TEST_PERSON_ID,
        at,
    }
}

async fn pool(url: &SecretString) -> Pool {
    crate::connections::connect_postgres(url, 4)
        .await
        .expect("Postgres accepts connections")
}

/// Step workers over `steps`, running until the token they answer is cancelled.
async fn running_workers(
    url: &SecretString,
    events: &EventBus,
    steps: Steps,
) -> (CancellationToken, tokio::task::JoinHandle<()>) {
    let workers = Workers::new(WorkersDeps {
        database: pool(url).await,
        events: events.clone(),
        activity: Activities::new(),
        steps,
    });
    let shutdown = CancellationToken::new();
    let running = workers.start(shutdown.clone());
    (shutdown, running)
}

/// Waits up to ten seconds for run `id` to reach `state`.
async fn until_run_is(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    state: WorkflowRunState,
) -> workflow_run::WorkflowRun {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let run = workflow_run::find(connection, id)
            .await
            .expect("the run reads");
        if run.state == state {
            return run;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "run {id} is {:?}, not {state:?}",
            run.state
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
