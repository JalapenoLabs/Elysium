// Copyright © 2026 Jalapeno Labs

//! `/api/v1/runs`: workflow runs, their steps and logs, and a person's actions on a step.
//!
//! Every route answers the company's members alone ([`MemberUser`](crate::auth::MemberUser)): a
//! run reads what was ingested (meetings, mail, messages), which a guest never reaches, as with
//! the knowledge base (`docs/knowledge.md`, Who sees what).
//!
//! Retrying, skipping, and resuming a step are writes: each records who did it in the run's
//! history, announces what moved, and signals the engine, whose step workers take what is due
//! at once. See `crate::workflows` and `docs/engine.md`, Runs.

mod get_run;
mod get_step_log;
mod list_runs;
mod resume_step;
mod retry_step;
mod skip_step;
#[cfg(test)]
mod tests;

use axum::Router;
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::step_job::{StepJob, StepJobState};
use crate::models::workflow_run::{self, HistoryEntry, Settled, WorkflowRun, WorkflowRunState};
use crate::realtime::Signal;
use crate::state::AppState;
use crate::workflows;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_runs::handle))
        .route("/{id}", get(get_run::handle))
        .route("/{id}/steps/{step_id}/log", get(get_step_log::handle))
        .route("/{id}/steps/{step_id}/retry", post(retry_step::handle))
        .route("/{id}/steps/{step_id}/skip", post(skip_step::handle))
        .route("/{id}/steps/{step_id}/resume", post(resume_step::handle))
}

/// A run as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunResponse {
    id: Uuid,
    /// The workflow's name in code, which the web app names in words.
    workflow: String,
    source_kind: String,
    source_id: String,
    state: WorkflowRunState,
    /// Why it stopped, while it is failed.
    error: Option<String>,
    /// Who or what started it: a person, or a machine user.
    created_by: Uuid,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
}

impl From<WorkflowRun> for RunResponse {
    fn from(run: WorkflowRun) -> Self {
        Self {
            id: run.id,
            workflow: run.workflow,
            source_kind: run.source_kind,
            source_id: run.source_id,
            state: run.state,
            error: run.error,
            created_by: run.created_by,
            created_at: run.created_at,
            started_at: run.started_at,
            finished_at: run.finished_at,
        }
    }
}

/// A step of a run as clients see it, without its log, which is read on its own.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepResponse {
    id: Uuid,
    run_id: Uuid,
    /// The step's name in code.
    step: String,
    state: StepJobState,
    /// The one line it collapses to, which the step wrote.
    summary: Option<String>,
    /// Why its last attempt failed.
    error: Option<String>,
    /// Attempts since it was last queued by its run or a person.
    attempts: i32,
    /// While it waits out a backoff, when it is tried again.
    available_at: Option<DateTime<Utc>>,
    /// When its last attempt started and ended.
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
}

impl From<StepJob> for StepResponse {
    fn from(step: StepJob) -> Self {
        Self {
            id: step.id,
            run_id: step.run_id,
            step: step.step,
            state: step.state,
            summary: step.summary,
            error: step.error,
            attempts: step.attempts,
            available_at: step.available_at,
            started_at: step.started_at,
            finished_at: step.finished_at,
        }
    }
}

/// A history entry as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntryResponse {
    id: Uuid,
    run_id: Uuid,
    /// The step it was about; `null` for the run's own entries.
    step_job_id: Option<Uuid>,
    /// `started`, `retried`, `skipped`, or `resumed`.
    kind: String,
    /// Who did it.
    created_by: Uuid,
    /// Shaped by `kind`; see `docs/engine.md`, Runs.
    data: Value,
    created_at: DateTime<Utc>,
}

impl From<HistoryEntry> for HistoryEntryResponse {
    fn from(entry: HistoryEntry) -> Self {
        Self {
            id: entry.id,
            run_id: entry.run_id,
            step_job_id: entry.step_job_id,
            kind: entry.kind,
            created_by: entry.created_by,
            data: entry.data,
            created_at: entry.created_at,
        }
    }
}

/// Run `run` as the run page shows it: the run, every step in order, and its history.
async fn detail(
    connection: &mut AsyncPgConnection,
    run: WorkflowRun,
    steps: Vec<StepJob>,
) -> Result<Value, ApiError> {
    let history: Vec<HistoryEntryResponse> = workflow_run::history(connection, run.id)
        .await?
        .into_iter()
        .map(HistoryEntryResponse::from)
        .collect();
    let steps: Vec<StepResponse> = steps.into_iter().map(StepResponse::from).collect();
    Ok(json!({
        "run": RunResponse::from(run),
        "steps": steps,
        "history": history,
    }))
}

/// Answers a person's action on a step: announces what it moved, tells the engine, whose
/// workers take a step it queued at once, and returns the run as it now stands.
async fn acted(
    state: &AppState,
    connection: &mut AsyncPgConnection,
    settled: Settled,
) -> Result<Value, ApiError> {
    workflows::announce(&state.events, &settled);
    state.events.signal(&Signal::Workflows);
    detail(connection, settled.run, settled.steps).await
}
