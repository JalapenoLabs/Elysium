// Copyright © 2026 Jalapeno Labs

//! Workflows, their steps, and their runs: the engine's background work, step by step. See
//! `docs/engine.md`, Workflows and steps, and Runs.
//!
//! - A **step** ([`Step`]) is one reusable unit of work with typed input and output, registered
//!   once by its name in [`Steps`]. It knows nothing of the workflows that use it.
//! - A **workflow** ([`Workflow`]) is steps declared in code ([`declare`]), each taking what the
//!   one before it gives, in whatever order and subset the problem needs.
//! - A **run** is one workflow on one source, started from its first step or entered at a later
//!   one ([`Workflow::new_run`], [`Workflow::new_run_at`], then [`start`]). Each of its steps is
//!   a row of its own (`crate::models::step_job`) that the engine's [`workers`] claim with
//!   `FOR UPDATE SKIP LOCKED`, run under a lease they renew, and record.
//!
//! The queue knows a run's steps by name alone, and carries each step's input and output as
//! JSON; [`Steps`] is where that JSON becomes a step's own types again, and where JSON that does
//! not fit fails the step with a reason rather than reaching it.
//!
//! Every change to a run reaches clients as `workflowRun.changed` and `stepJob.changed`
//! ([`announce`]), carrying ids and states alone: a run's input, output, and logs hold what it
//! read, which clients load by id.

mod step;
#[cfg(test)]
mod tests;
pub mod workers;
mod workflow;

use chrono::Utc;
use diesel::QueryResult;
use diesel_async::AsyncPgConnection;

pub use self::step::{Done, Step, StepContext, StepFailure, StepLog, Steps};
pub use self::workflow::{Outline, Start, Workflow, declare};
use crate::models::workflow_run::{self, NewRun, Settled, Started};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::{company_doc, conversations, meetings};

/// Every workflow this build runs. It is the one list the engine checks the steps it registers
/// against as it starts ([`Steps::assert_runs`], [`Steps::assert_declared_in`]), and the one the
/// model settings page lists steps from (`crate::agent_models::catalog`), so a step can neither
/// run unlisted nor be listed without running. A new workflow is added here.
pub fn declared() -> Vec<Outline> {
    vec![
        meetings::workflow::workflow().outline(),
        company_doc::workflow::workflow().outline(),
        conversations::workflow::workflow().outline(),
    ]
}

/// Starts `new` and tells the engine, unless the workflow already has a live run on the source,
/// which is answered instead. See [`workflow_run::start`].
///
/// # Errors
/// Propagates any database error.
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

/// Whether `name` can name a workflow or a step as the queue stores them: `snake_case` from a
/// letter, at most 64 characters, as the columns' checks hold them.
fn is_name(name: &str) -> bool {
    let mut characters = name.chars();
    let starts_with_letter = characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase());
    starts_with_letter
        && name.len() <= 64
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}
