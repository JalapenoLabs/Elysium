// Copyright © 2026 Jalapeno Labs

//! Workflow runs: one workflow on one source, its steps in order, and its history.
//!
//! A run is started with the names of its steps ([`start`]); each becomes a row in `step_jobs`
//! (`crate::models::step_job`), which the engine claims and runs. Steps are linked by their
//! data alone: the first receives the run's input, and each after it the output of the step
//! before it, or that step's own input when someone skipped it.
//!
//! Every write to a run or one of its steps locks the run's row first, then changes the step,
//! then [settles](settle) the run in the same transaction: it queues the next step the run has
//! reached, and derives the run's state from its steps. Taking the run's lock first, always,
//! keeps the engine's writes and a person's from deadlocking; the engine's claim, which locks
//! steps alone, never waits on a run.
//!
//! What a person does to a run (it started, a step retried, skipped, or resumed) is recorded in
//! its history (`workflow_run_events`) in the same transaction, with who did it.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::database::schema::{step_jobs, workflow_run_events, workflow_runs};
use crate::models::step_job::{StepJob, StepJobState};

/// Where a run stands, derived from its steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize)]
#[ExistingTypePath = "crate::database::schema::sql_types::WorkflowRunState"]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunState {
    /// No step has started yet.
    Queued,
    /// A step runs, waits to be claimed, or waits out its backoff.
    Running,
    /// A step asks a person to look before the run goes on.
    Held,
    /// A step failed for good; the run waits for someone to retry or skip it.
    Failed,
    /// Every step succeeded or was skipped.
    Succeeded,
}

impl WorkflowRunState {
    /// The name clients receive, as the state's JSON says it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Held => "held",
            Self::Failed => "failed",
            Self::Succeeded => "succeeded",
        }
    }

    const fn is_over(self) -> bool {
        matches!(self, Self::Failed | Self::Succeeded)
    }
}

/// A stored run.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = workflow_runs, check_for_backend(diesel::pg::Pg))]
pub struct WorkflowRun {
    pub id: Uuid,
    pub workflow: String,
    pub source_kind: String,
    pub source_id: String,
    pub state: WorkflowRunState,
    pub input: Value,
    /// The failed step's reason; set exactly while the run is failed.
    pub error: Option<String>,
    /// Who or what started it.
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

/// A run to start.
#[derive(Debug, Clone)]
pub struct NewRun {
    /// The workflow's name in code, `snake_case`.
    pub workflow: String,
    /// What the run is about, such as `meeting`, and that thing's id.
    pub source_kind: String,
    pub source_id: String,
    /// What the first step receives.
    pub input: Value,
    /// Its steps' names, in the order they run.
    pub steps: Vec<String>,
    /// Who or what starts it: a person, or a machine user for what Elysium starts itself.
    pub created_by: Uuid,
}

/// What a history entry records, and so how its `data` is shaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryKind {
    /// The run was started. `data` is `{ "steps": [names] }`.
    Started,
    /// A failed step was queued again. `data` is `{ "error" }`, the reason it had failed with.
    Retried,
    /// A step was skipped. `data` is `{ "from" }`, the state it was skipped from.
    Skipped,
    /// A held step was let through, its output passed on. `data` is empty.
    Resumed,
}

impl HistoryKind {
    /// The name stored in `kind` and sent to clients.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Retried => "retried",
            Self::Skipped => "skipped",
            Self::Resumed => "resumed",
        }
    }
}

/// A stored history entry.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = workflow_run_events, check_for_backend(diesel::pg::Pg))]
pub struct HistoryEntry {
    pub id: Uuid,
    pub run_id: Uuid,
    /// The step it was about; `None` for the run's own entries.
    pub step_job_id: Option<Uuid>,
    /// A [`HistoryKind`] name, kept as text so entries a later version wrote still load.
    pub kind: String,
    pub created_by: Uuid,
    pub data: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = workflow_run_events)]
struct HistoryRow {
    id: Uuid,
    run_id: Uuid,
    step_job_id: Option<Uuid>,
    kind: &'static str,
    created_by: Uuid,
    data: Value,
    created_at: DateTime<Utc>,
}

/// Who changes a run, and when.
#[derive(Debug, Clone, Copy)]
pub struct Actor {
    pub user_id: Uuid,
    pub at: DateTime<Utc>,
}

/// A run as a write left it: the run, all its steps in order, and which steps the write moved.
#[derive(Debug, Clone)]
pub struct Settled {
    pub run: WorkflowRun,
    pub steps: Vec<StepJob>,
    /// The steps whose state the write changed, the step it acted on and any it queued.
    pub moved: Vec<Uuid>,
}

impl Settled {
    /// The moved steps, as they now stand.
    pub fn moved_steps(&self) -> impl Iterator<Item = &StepJob> {
        self.steps
            .iter()
            .filter(|step| self.moved.contains(&step.id))
    }
}

/// The outcome of [`start`].
#[derive(Debug, Clone)]
pub struct Started {
    pub settled: Settled,
    /// False when the workflow already had a live run on the source, which is answered
    /// instead of starting another.
    pub created: bool,
}

#[derive(Insertable)]
#[diesel(table_name = workflow_runs)]
struct RunRow<'a> {
    id: Uuid,
    workflow: &'a str,
    source_kind: &'a str,
    source_id: &'a str,
    input: &'a Value,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = step_jobs)]
struct StepRow<'a> {
    id: Uuid,
    run_id: Uuid,
    position: i32,
    step: &'a str,
}

/// Starts `new`, its first step queued at once. A workflow has one live run per source, so when
/// one has not yet succeeded it is answered instead (`created` false): whatever starts runs may
/// do so every time it notices the source.
///
/// # Errors
/// Propagates any database error, including a check violation for a name that is not
/// `snake_case` or more steps than a run's positions hold.
///
/// # Panics
/// Panics when `new` names no step, which only a programming error can cause.
pub async fn start(
    connection: &mut AsyncPgConnection,
    new: NewRun,
    at: DateTime<Utc>,
) -> QueryResult<Started> {
    assert!(!new.steps.is_empty(), "a run has at least one step");
    connection
        .transaction(async move |connection| {
            let run_id = Uuid::now_v7();
            let inserted = diesel::insert_into(workflow_runs::table)
                .values(RunRow {
                    id: run_id,
                    workflow: &new.workflow,
                    source_kind: &new.source_kind,
                    source_id: &new.source_id,
                    input: &new.input,
                    created_by: new.created_by,
                    created_at: at,
                })
                .on_conflict_do_nothing()
                .execute(connection)
                .await?;
            if inserted == 0 {
                // The insert met the one live run per source, which this transaction now sees.
                let live = find_live(connection, &new.workflow, &new.source_kind, &new.source_id)
                    .await?
                    .ok_or(diesel::result::Error::NotFound)?;
                let steps = steps_of(connection, live.id).await?;
                return Ok(Started {
                    settled: Settled {
                        run: live,
                        steps,
                        moved: Vec::new(),
                    },
                    created: false,
                });
            }

            let mut rows = Vec::with_capacity(new.steps.len());
            for (position, step) in new.steps.iter().enumerate() {
                rows.push(StepRow {
                    id: Uuid::now_v7(),
                    run_id,
                    // A position past i32 fails the row's check rather than wrapping.
                    position: i32::try_from(position).unwrap_or(-1),
                    step,
                });
            }
            diesel::insert_into(step_jobs::table)
                .values(&rows)
                .execute(connection)
                .await?;
            record(
                connection,
                run_id,
                None,
                HistoryKind::Started,
                Actor {
                    user_id: new.created_by,
                    at,
                },
                serde_json::json!({ "steps": new.steps }),
            )
            .await?;
            let run = lock(connection, run_id).await?;
            let settled = settle(connection, run, Vec::new(), at).await?;
            Ok(Started {
                settled,
                created: true,
            })
        })
        .await
}

/// The run of `workflow` on a source that has not yet succeeded, if there is one: a workflow has
/// one live run per source.
///
/// # Errors
/// Propagates any database error.
pub async fn find_live(
    connection: &mut AsyncPgConnection,
    workflow: &str,
    source_kind: &str,
    source_id: &str,
) -> QueryResult<Option<WorkflowRun>> {
    workflow_runs::table
        .filter(workflow_runs::workflow.eq(workflow))
        .filter(workflow_runs::source_kind.eq(source_kind))
        .filter(workflow_runs::source_id.eq(source_id))
        .filter(workflow_runs::state.ne(WorkflowRunState::Succeeded))
        .select(WorkflowRun::as_select())
        .first(connection)
        .await
        .optional()
}

/// The newest run of `workflow` on a source, live or finished, if it ever ran: for a schedule
/// that runs a workflow again some time after its last run started.
///
/// # Errors
/// Propagates any database error.
pub async fn newest(
    connection: &mut AsyncPgConnection,
    workflow: &str,
    source_kind: &str,
    source_id: &str,
) -> QueryResult<Option<WorkflowRun>> {
    workflow_runs::table
        .filter(workflow_runs::workflow.eq(workflow))
        .filter(workflow_runs::source_kind.eq(source_kind))
        .filter(workflow_runs::source_id.eq(source_id))
        .order(workflow_runs::id.desc())
        .select(WorkflowRun::as_select())
        .first(connection)
        .await
        .optional()
}

/// Run `id`.
///
/// # Errors
/// `NotFound` for an unknown run.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<WorkflowRun> {
    workflow_runs::table
        .find(id)
        .select(WorkflowRun::as_select())
        .first(connection)
        .await
}

/// Up to `limit` runs, newest first, older than run `before` when it is given.
///
/// # Errors
/// Propagates any database error.
pub async fn list(
    connection: &mut AsyncPgConnection,
    before: Option<Uuid>,
    limit: i64,
) -> QueryResult<Vec<WorkflowRun>> {
    let mut query = workflow_runs::table
        .select(WorkflowRun::as_select())
        .order(workflow_runs::id.desc())
        .limit(limit)
        .into_boxed();
    if let Some(before) = before {
        query = query.filter(workflow_runs::id.lt(before));
    }
    query.load(connection).await
}

/// Every step of run `run_id`, in the order they run.
///
/// # Errors
/// Propagates any database error.
pub async fn steps_of(
    connection: &mut AsyncPgConnection,
    run_id: Uuid,
) -> QueryResult<Vec<StepJob>> {
    step_jobs::table
        .filter(step_jobs::run_id.eq(run_id))
        .order(step_jobs::position)
        .select(StepJob::as_select())
        .load(connection)
        .await
}

/// Run `run_id`'s history, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn history(
    connection: &mut AsyncPgConnection,
    run_id: Uuid,
) -> QueryResult<Vec<HistoryEntry>> {
    workflow_run_events::table
        .filter(workflow_run_events::run_id.eq(run_id))
        .order(workflow_run_events::id)
        .select(HistoryEntry::as_select())
        .load(connection)
        .await
}

/// Locks run `id`'s row until the transaction ends. Every write to a run or its steps takes
/// this first.
///
/// # Errors
/// `NotFound` for an unknown run.
pub(crate) async fn lock(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<WorkflowRun> {
    workflow_runs::table
        .find(id)
        .for_update()
        .select(WorkflowRun::as_select())
        .first(connection)
        .await
}

/// Records a history entry for run `run_id`, in the caller's transaction.
pub(crate) async fn record(
    connection: &mut AsyncPgConnection,
    run_id: Uuid,
    step_job_id: Option<Uuid>,
    kind: HistoryKind,
    actor: Actor,
    data: Value,
) -> QueryResult<()> {
    diesel::insert_into(workflow_run_events::table)
        .values(HistoryRow {
            id: Uuid::now_v7(),
            run_id,
            step_job_id,
            kind: kind.as_str(),
            created_by: actor.user_id,
            data,
            created_at: actor.at,
        })
        .execute(connection)
        .await
        .map(drop)
}

/// Settles `run`, locked by the caller, after a write to its steps: queues the next step it has
/// reached, then derives its state from its steps. `moved` names the steps the caller changed;
/// the answer adds any this queued.
///
/// # Errors
/// Propagates any database error.
pub(crate) async fn settle(
    connection: &mut AsyncPgConnection,
    run: WorkflowRun,
    mut moved: Vec<Uuid>,
    at: DateTime<Utc>,
) -> QueryResult<Settled> {
    let mut steps = steps_of(connection, run.id).await?;
    if let Some((index, input)) = next_to_queue(&run.input, &steps) {
        let queued = diesel::update(step_jobs::table.find(steps[index].id))
            .set((
                step_jobs::state.eq(StepJobState::Queued),
                step_jobs::input.eq(input),
                step_jobs::available_at.eq(at),
            ))
            .returning(StepJob::as_returning())
            .get_result(connection)
            .await?;
        moved.push(queued.id);
        steps[index] = queued;
    }

    let state = run_state_of(&steps);
    let error = steps
        .iter()
        .find(|step| step.state == StepJobState::Failed)
        .and_then(|step| step.error.clone());
    let started_at = match (run.started_at, state) {
        (None, WorkflowRunState::Queued) => None,
        (None, _) => Some(at),
        (started, _) => started,
    };
    let finished_at = match (run.finished_at, state.is_over()) {
        (_, false) => None,
        (None, true) => Some(at),
        (finished, true) => finished,
    };
    let unchanged = run.state == state
        && run.error == error
        && run.started_at == started_at
        && run.finished_at == finished_at;
    let run = if unchanged {
        run
    } else {
        diesel::update(workflow_runs::table.find(run.id))
            .set((
                workflow_runs::state.eq(state),
                workflow_runs::error.eq(error),
                workflow_runs::started_at.eq(started_at),
                workflow_runs::finished_at.eq(finished_at),
            ))
            .returning(WorkflowRun::as_returning())
            .get_result(connection)
            .await?
    };
    Ok(Settled { run, steps, moved })
}

/// The step a run has reached but not queued, by its index in `steps`, with the input it
/// receives: walking the steps in order, the run's input passes through each skipped step and
/// is replaced by each succeeded step's output; the first pending step met receives it. Any
/// other step stops the walk, since the run waits on it.
fn next_to_queue(run_input: &Value, steps: &[StepJob]) -> Option<(usize, Value)> {
    let mut carried = run_input;
    for (index, step) in steps.iter().enumerate() {
        match step.state {
            StepJobState::Skipped => {}
            StepJobState::Succeeded => {
                // The row's check keeps output set on every succeeded step.
                carried = step.output.as_ref()?;
            }
            StepJobState::Pending => return Some((index, carried.clone())),
            StepJobState::Queued
            | StepJobState::Running
            | StepJobState::Held
            | StepJobState::Failed => return None,
        }
    }
    None
}

/// A run's state, from its steps: failed while any step failed, held while any is held,
/// succeeded once every step succeeded or was skipped, queued until any step has started or
/// finished, and running otherwise.
fn run_state_of(steps: &[StepJob]) -> WorkflowRunState {
    let mut held = false;
    let mut done = true;
    let mut begun = false;
    for step in steps {
        match step.state {
            StepJobState::Failed => return WorkflowRunState::Failed,
            StepJobState::Held => held = true,
            StepJobState::Succeeded | StepJobState::Skipped => begun = true,
            StepJobState::Running => {
                begun = true;
                done = false;
            }
            StepJobState::Pending | StepJobState::Queued => {
                begun |= step.started_at.is_some();
                done = false;
            }
        }
    }
    if held {
        return WorkflowRunState::Held;
    }
    if done {
        return WorkflowRunState::Succeeded;
    }
    if begun {
        return WorkflowRunState::Running;
    }
    WorkflowRunState::Queued
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn step(state: StepJobState, output: Option<Value>) -> StepJob {
        StepJob {
            id: Uuid::now_v7(),
            run_id: Uuid::nil(),
            position: 0,
            step: "a_step".to_owned(),
            state,
            input: None,
            output,
            summary: None,
            error: None,
            attempts: 0,
            available_at: None,
            started_at: None,
            finished_at: None,
        }
    }

    #[test]
    fn the_first_pending_step_receives_the_last_output_carried_past_skipped_steps() {
        let input = json!({ "meeting": "m-1" });
        let steps = [
            step(StepJobState::Succeeded, Some(json!({ "transcript": "t" }))),
            step(StepJobState::Skipped, None),
            step(StepJobState::Pending, None),
            step(StepJobState::Pending, None),
        ];

        assert_eq!(
            next_to_queue(&input, &steps),
            Some((2, json!({ "transcript": "t" })))
        );
    }

    #[test]
    fn a_skipped_first_step_passes_the_runs_input_on() {
        let input = json!({ "meeting": "m-1" });
        let steps = [
            step(StepJobState::Skipped, None),
            step(StepJobState::Pending, None),
        ];

        assert_eq!(next_to_queue(&input, &steps), Some((1, input.clone())));
    }

    #[test]
    fn nothing_is_queued_while_the_run_waits_on_a_step() {
        let input = json!({});
        for waiting in [
            StepJobState::Queued,
            StepJobState::Running,
            StepJobState::Held,
            StepJobState::Failed,
        ] {
            let steps = [step(waiting, None), step(StepJobState::Pending, None)];
            assert_eq!(next_to_queue(&input, &steps), None, "{waiting:?}");
        }
        let finished = [step(StepJobState::Succeeded, Some(json!({})))];
        assert_eq!(next_to_queue(&input, &finished), None);
    }

    #[test]
    fn a_run_is_failed_or_held_by_any_step_and_succeeds_only_when_every_step_is_done() {
        let done = || step(StepJobState::Succeeded, Some(json!({})));
        let state = |steps: &[StepJob]| run_state_of(steps);

        assert_eq!(
            state(&[done(), step(StepJobState::Failed, None)]),
            WorkflowRunState::Failed
        );
        assert_eq!(
            state(&[done(), step(StepJobState::Held, Some(json!({})))]),
            WorkflowRunState::Held
        );
        assert_eq!(
            state(&[done(), step(StepJobState::Skipped, None)]),
            WorkflowRunState::Succeeded
        );
        assert_eq!(
            state(&[done(), step(StepJobState::Queued, None)]),
            WorkflowRunState::Running
        );
    }

    #[test]
    fn a_run_is_queued_until_a_step_has_started() {
        let mut first = step(StepJobState::Queued, None);
        let steps = [first.clone(), step(StepJobState::Pending, None)];
        assert_eq!(run_state_of(&steps), WorkflowRunState::Queued);

        // Queued again for a retry, after an attempt that started.
        first.started_at = Some(Utc::now());
        let steps = [first, step(StepJobState::Pending, None)];
        assert_eq!(run_state_of(&steps), WorkflowRunState::Running);
    }
}
