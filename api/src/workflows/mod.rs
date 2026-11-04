// Copyright © 2026 Jalapeno Labs

//! Workflow runs: the engine's queue of background work, step by step. See `docs/engine.md`,
//! Runs.
//!
//! A run is one workflow on one source, started with the names of its steps ([`start`]). Each
//! step is a row of its own (`crate::models::step_job`) that the engine's [`workers`] claim with
//! `FOR UPDATE SKIP LOCKED`, run under a lease they renew, and record. A step is found by its
//! name in [`Steps`], which hands it a [`StepCall`] (its input as JSON and a [`StepLog`] to write
//! to) and takes back a [`StepOutcome`]. Steps are linked by their data alone: each receives the
//! output of the one before it.
//!
//! This layer knows nothing of what any step does, or which workflows exist: the step and
//! workflow framework builds on it (`docs/engine.md`, Workflows and steps), registering its
//! steps here by name and starting runs with [`start`].
//!
//! Every change to a run reaches clients as `workflowRun.changed` and `stepJob.changed`
//! ([`announce`]), carrying ids and states alone: a run's input, output, and logs hold what it
//! read, which clients load by id.

pub mod workers;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use chrono::Utc;
use diesel::QueryResult;
use diesel_async::AsyncPgConnection;
use serde_json::Value;
use uuid::Uuid;

pub use crate::models::step_job::StepOutcome;
use crate::models::workflow_run::{self, NewRun, Settled, Started};
use crate::realtime::{EventBus, ServerEvent, Signal};

/// What one attempt at a step is given.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the step and workflow framework (#76) writes the first steps"
    )
)]
#[derive(Debug, Clone)]
pub struct StepCall {
    pub run_id: Uuid,
    pub step_job_id: Uuid,
    /// The run's workflow, and what it is about.
    pub workflow: String,
    pub source_kind: String,
    pub source_id: String,
    /// The step's own name, the one it was found by.
    pub step: String,
    /// The run's input for its first step; else the output of the step before.
    pub input: Value,
    /// Which attempt this is, from 1. An attempt after the first may follow one that did some
    /// of the work before its engine stopped, so a step does its work idempotently.
    pub attempt: i32,
    /// Where the step writes what it does, for the Runs page.
    pub log: StepLog,
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
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the step and workflow framework (#76) writes the first steps"
        )
    )]
    pub fn line(&self, text: &str) {
        let mut pending = self.lock();
        pending.push_str(text.trim_end_matches('\n'));
        pending.push('\n');
    }

    /// Everything written since the last take.
    fn take(&self) -> String {
        std::mem::take(&mut *self.lock())
    }

    /// Puts back `text`, taken but not stored, ahead of whatever was written since.
    fn put_back(&self, text: String) {
        let mut pending = self.lock();
        let written_since = std::mem::replace(&mut *pending, text);
        pending.push_str(&written_since);
    }

    /// The text is whole after every push, so a poisoned lock holds nothing half written.
    fn lock(&self) -> std::sync::MutexGuard<'_, String> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A step's attempt, as the registry hands it to a worker.
type StepFuture = Pin<Box<dyn Future<Output = StepOutcome> + Send>>;

/// A step as it is registered: given an attempt, it runs it.
type Step = dyn Fn(StepCall) -> StepFuture + Send + Sync;

/// Every step this build can run, by name. Built once at startup, then shared by every worker.
#[derive(Clone, Default)]
pub struct Steps {
    by_name: HashMap<&'static str, Arc<Step>>,
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

    /// Registers `step` under `name`, the name runs list it by.
    ///
    /// # Panics
    /// Panics when `name` is registered already, which only a programming error can cause.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the step and workflow framework (#76) registers the first steps"
        )
    )]
    pub fn register<Run, Attempt>(&mut self, name: &'static str, step: Run)
    where
        Run: Fn(StepCall) -> Attempt + Send + Sync + 'static,
        Attempt: Future<Output = StepOutcome> + Send + 'static,
    {
        let boxed: Arc<Step> = Arc::new(move |call| Box::pin(step(call)) as StepFuture);
        let previous = self.by_name.insert(name, boxed);
        assert!(previous.is_none(), "step {name} is registered twice");
    }

    /// The step named `name`, if this build has it.
    fn find(&self, name: &str) -> Option<Arc<Step>> {
        self.by_name.get(name).cloned()
    }
}

/// Starts `new` and tells the engine, unless the workflow already has a live run on the source,
/// which is answered instead. See [`workflow_run::start`].
///
/// # Errors
/// Propagates any database error.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the step and workflow framework (#76) starts the first runs"
    )
)]
pub async fn start(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    new: NewRun,
) -> QueryResult<Started> {
    let started = workflow_run::start(connection, new, Utc::now()).await?;
    if started.created {
        announce(events, &started.settled);
        events.signal(&Signal::Workflows);
    }
    Ok(started)
}

/// Tells every client that `settled`'s run changed, and each step the write moved.
pub fn announce(events: &EventBus, settled: &Settled) {
    events.publish(&ServerEvent::WorkflowRunChanged {
        id: settled.run.id,
        state: settled.run.state,
    });
    for step in settled.moved_steps() {
        events.publish(&ServerEvent::StepJobChanged {
            id: step.id,
            run_id: step.run_id,
            state: step.state,
        });
    }
}
