// Copyright © 2026 Jalapeno Labs

//! Steps: one reusable unit of work each, with typed input and output, found by name.
//!
//! A step is a type implementing [`Step`]: its name, the input it takes, the output it gives,
//! and how one attempt runs. It knows nothing of the workflows that use it, or what runs before
//! or after it; it is handed its input and a [`StepContext`] (its run's ids, which attempt this
//! is, a log, and its model setting), and answers [`Done`] or a [`StepFailure`].
//!
//! Every step this build runs is registered once, by its name, in [`Steps`], which the engine's
//! workers find each claimed step in. Registering is where a step meets the queue: its input
//! arrives as JSON stored on its row, and is read as the step's own input type before the step
//! runs. JSON that does not read as that type, which a person skipping the step before it can
//! cause, fails the step for good with a reason naming it, rather than reaching the step.

use std::any::TypeId;
use std::collections::HashMap;
use std::fmt::Display;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use uuid::Uuid;

use super::Workflow;
use crate::database::Pool;
use crate::models::step_job::StepOutcome;
use crate::models::step_model_setting::{self, ModelChoice};

/// One reusable unit of a workflow's work, such as fetching a transcript.
///
/// A step is linked to the steps around it by its types alone: a workflow may follow it with any
/// step whose input is its output
/// ([`WorkflowBuilder::then`](super::workflow::WorkflowBuilder::then)). One instance runs every
/// attempt, side by side, so it holds what it needs (a database pool, an HTTP client, the
/// runner) and keeps no state of one attempt.
///
/// A step a person may want to skip, such as correcting a transcript, is best written to take
/// and give one type, so skipping it hands the next step what it reads. Otherwise the next step
/// fails, saying why.
///
/// An attempt may run more than once for one success, when its engine stops mid-step, so a step
/// does its work idempotently: keyed by [`StepContext::step_job_id`], or by what its input names.
pub trait Step: Send + Sync + 'static {
    /// The step's name in code, `snake_case`: what runs list it by, and the Runs page shows in
    /// words. Never a number, and never changed once runs name it, since a run stores the names
    /// of its steps and a build without one fails that step.
    const NAME: &'static str;

    /// What it takes. Serialized when a run starts at this step, and read back from the queue
    /// before each attempt.
    type Input: Serialize + DeserializeOwned + Send + 'static;

    /// What it gives, which the step after it takes as its input.
    type Output: Serialize + Send + 'static;

    /// Runs one attempt on `input`.
    fn run(
        &self,
        input: Self::Input,
        context: StepContext,
    ) -> impl Future<Output = Result<Done<Self::Output>, StepFailure>> + Send;
}

/// How an attempt that did its work ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done<Output> {
    /// `output` is what the next step takes; `summary` is the one line its row collapses to.
    Succeeded {
        output: Output,
        summary: Option<String>,
    },
    /// It asks a person to look before the run goes on, such as approving a corrected
    /// transcript. `summary` says what to look at; resuming passes `output` on.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "correction (#79) is the first step that holds its run for approval"
        )
    )]
    Held { output: Output, summary: String },
    /// It found nothing to do, such as a meeting with no recording to transcribe. `reason` is
    /// the line its row collapses to. Like a person's skip, it gives no output: the step after
    /// it receives this step's own input, so a step that may skip itself takes and gives one
    /// type, or gives a type its input also reads as.
    Skipped { reason: String },
}

/// Why an attempt could not do its work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepFailure {
    /// Shown on the Runs page, and written to the step's log.
    pub reason: String,
    /// Whether another attempt may succeed, such as after a provider that did not answer. A
    /// retryable failure is tried again after a backoff while attempts remain; any other fails
    /// the step at once, for a person to retry or skip.
    pub retryable: bool,
}

impl StepFailure {
    /// A failure another attempt may get past, such as a provider's timeout.
    pub fn retryable(reason: impl Display) -> Self {
        Self {
            reason: format!("{reason:#}"),
            retryable: true,
        }
    }

    /// A failure every attempt would repeat, such as a meeting with no transcript.
    pub fn permanent(reason: impl Display) -> Self {
        Self {
            reason: format!("{reason:#}"),
            retryable: false,
        }
    }
}

/// What an attempt is given besides its input.
#[derive(Clone)]
pub struct StepContext {
    /// The run it is part of, and its own row: the keys a step's idempotent work is filed under.
    pub run_id: Uuid,
    pub step_job_id: Uuid,
    /// Which attempt this is, from 1. An attempt after the first may follow one that did some of
    /// the work before its engine stopped.
    pub attempt: i32,
    /// Where the step writes what it does, for the Runs page.
    pub log: StepLog,
    /// The step's registered name, which its model setting is filed under.
    pub(super) step: &'static str,
    pub(super) database: Pool,
}

impl std::fmt::Debug for StepContext {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StepContext")
            .field("run_id", &self.run_id)
            .field("step_job_id", &self.step_job_id)
            .field("attempt", &self.attempt)
            .field("step", &self.step)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
impl StepContext {
    /// A context for calling the step named `step` directly, outside the queue, as the first
    /// attempt of a run of its own.
    pub fn for_test(database: Pool, step: &'static str) -> Self {
        Self {
            run_id: Uuid::now_v7(),
            step_job_id: Uuid::now_v7(),
            attempt: 1,
            log: StepLog::default(),
            step,
            database,
        }
    }
}

impl StepContext {
    /// The model and effort this step's agent turn runs on, read from its global setting now, as
    /// the turn starts, so an admin's change applies from the next turn. Written to the log, so
    /// the Runs page says which model did the work.
    ///
    /// # Errors
    /// A retryable failure when the setting cannot be read.
    pub async fn model(&self) -> Result<ModelChoice, StepFailure> {
        let read = async {
            let mut connection = self.database.get().await?;
            anyhow::Ok(step_model_setting::for_step(&mut connection, self.step).await?)
        };
        let choice = read.await.map_err(|error| {
            StepFailure::retryable(format!(
                "Could not read this step's model setting: {error:#}"
            ))
        })?;
        self.log.line(&format!(
            "Runs on {} at {} effort",
            choice.model,
            choice.effort.as_str()
        ));
        Ok(choice)
    }
}

/// A step's log as it runs. What it writes is kept on the step's row within a second, and every
/// attempt's is kept, the newest 64 KiB of it. Cheap to clone; clones write to one log.
#[derive(Debug, Clone, Default)]
pub struct StepLog {
    /// Written but not yet stored; the worker running the step takes it.
    pending: Arc<Mutex<String>>,
}

impl StepLog {
    /// Writes `text` as one line.
    pub fn line(&self, text: &str) {
        let mut pending = self.lock();
        pending.push_str(text.trim_end_matches('\n'));
        pending.push('\n');
    }

    /// Everything written and not yet stored, left in place.
    #[cfg(test)]
    pub fn unstored(&self) -> String {
        self.lock().clone()
    }

    /// Everything written since the last take.
    pub(super) fn take(&self) -> String {
        std::mem::take(&mut *self.lock())
    }

    /// Puts back `text`, taken but not stored, ahead of whatever was written since.
    pub(super) fn put_back(&self, text: String) {
        let mut pending = self.lock();
        let written_since = std::mem::replace(&mut *pending, text);
        pending.push_str(&written_since);
    }

    /// The text is whole after every push, so a poisoned lock holds nothing half written.
    fn lock(&self) -> std::sync::MutexGuard<'_, String> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// An attempt at a registered step, as a worker drives it.
pub(super) type StepFuture = Pin<Box<dyn Future<Output = StepOutcome> + Send>>;

/// A registered step with its types erased: given the JSON its row holds, it runs an attempt.
pub(super) type ErasedStep = dyn Fn(Value, StepContext) -> StepFuture + Send + Sync;

/// A step as [`Steps`] holds it.
struct Registered {
    /// The step's type, so a workflow naming a different type by the same name is caught.
    type_id: TypeId,
    run: Arc<ErasedStep>,
}

/// Every step this build can run, by name. Built once at startup, then shared by every worker.
#[derive(Clone, Default)]
pub struct Steps {
    by_name: HashMap<&'static str, Arc<Registered>>,
}

impl std::fmt::Debug for Steps {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut names: Vec<&&str> = self.by_name.keys().collect();
        names.sort();
        formatter
            .debug_struct("Steps")
            .field("names", &names)
            .finish()
    }
}

impl Steps {
    /// No steps at all: every run's steps fail as unknown until steps are registered.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `step` under its name, the name runs list it by.
    ///
    /// # Panics
    /// Panics when its name is not `snake_case` or is registered already, which only a
    /// programming error can cause.
    pub fn register<Registering: Step>(&mut self, step: Registering) {
        let name = Registering::NAME;
        assert!(
            super::is_name(name),
            "step {name} is not named in snake_case"
        );
        let step = Arc::new(step);
        let run: Arc<ErasedStep> = Arc::new(move |input, context| {
            let step = Arc::clone(&step);
            Box::pin(async move { attempt(&*step, input, context).await }) as StepFuture
        });
        let registered = Arc::new(Registered {
            type_id: TypeId::of::<Registering>(),
            run,
        });
        let previous = self.by_name.insert(name, registered);
        assert!(previous.is_none(), "step {name} is registered twice");
    }

    /// Checks that every step `workflow` names is registered, as the type it declares, so a
    /// workflow and the steps registered for it cannot drift apart unnoticed.
    ///
    /// # Panics
    /// Panics naming the first step that is missing or registered as another type.
    pub fn assert_runs<Input>(&self, workflow: &Workflow<Input>) {
        for declared in workflow.declared_steps() {
            let registered = self.by_name.get(declared.name).unwrap_or_else(|| {
                panic!(
                    "workflow {} names step {}, which is not registered",
                    workflow.name(),
                    declared.name
                )
            });
            assert!(
                registered.type_id == declared.type_id,
                "workflow {} names step {} as a type other than the one registered by that name",
                workflow.name(),
                declared.name
            );
        }
    }

    /// The step named `name`, under the name it was registered by, if this build has it.
    pub(super) fn find(&self, name: &str) -> Option<(&'static str, Arc<ErasedStep>)> {
        self.by_name
            .get_key_value(name)
            .map(|(name, registered)| (*name, Arc::clone(&registered.run)))
    }
}

/// One attempt at `step`, at the queue's boundary: reads the JSON on its row as its input type,
/// runs it, and writes its output back as JSON. Input that does not read as its type fails it
/// for good, since every attempt would read the same JSON.
async fn attempt<Attempted: Step>(
    step: &Attempted,
    input: Value,
    context: StepContext,
) -> StepOutcome {
    let name = Attempted::NAME;
    let input = match serde_json::from_value::<Attempted::Input>(input) {
        Ok(input) => input,
        Err(error) => {
            return StepOutcome::Failed {
                reason: format!(
                    "The input {name} received is not what it takes ({error}). A step before it \
                     that was skipped would cause this, since {name} takes what that step gives."
                ),
                retryable: false,
            };
        }
    };
    let done = match step.run(input, context).await {
        Ok(done) => done,
        Err(failure) => {
            return StepOutcome::Failed {
                reason: failure.reason,
                retryable: failure.retryable,
            };
        }
    };
    match done {
        Done::Succeeded { output, summary } => match serde_json::to_value(output) {
            Ok(output) => StepOutcome::Succeeded { output, summary },
            Err(error) => unstorable(name, &error),
        },
        Done::Held { output, summary } => match serde_json::to_value(output) {
            Ok(output) => StepOutcome::Held { output, summary },
            Err(error) => unstorable(name, &error),
        },
        Done::Skipped { reason } => StepOutcome::Skipped { reason },
    }
}

/// How an attempt ends whose output JSON cannot hold, which only an output type such as a map
/// keyed by numbers causes. Every attempt would give the same, so it fails for good.
fn unstorable(name: &str, error: &serde_json::Error) -> StepOutcome {
    StepOutcome::Failed {
        reason: format!("{name} gave an output that cannot be stored as JSON: {error}"),
        retryable: false,
    }
}
