// Copyright © 2026 Jalapeno Labs

//! Work batches: what a Project Manager submits for the engine's worker pool. See
//! `docs/engine.md`, Workers and the pool.
//!
//! A batch is a goal a worker can work from alone, the repositories it needs, a priority (ASAP,
//! normal, or a scheduled time), and its provenance: the Project Manager's turn that submitted
//! it, the timeline records that called for it, and the action items it works on.
//!
//! # Where a batch stands
//!
//! A batch waits `queued` until the pool gives it a slot (`assigned`, [`assign`]), and a worker
//! starts on it (`running`, [`start`]) and ends it `succeeded` or `failed` ([`finish`]), which
//! wakes its project's Project Manager in the same transaction. While it waits for a worker to
//! start it, a person, Elysia, or its Project Manager may hold it back (`paused`) or withdraw it
//! (`cancelled`, [`cancel`]).
//!
//! # Slots and their leases
//!
//! A batch holds a slot exactly while it is `assigned` or `running`, and exactly then it is
//! claimed: a `claim_token` naming the engine that holds the slot, under a [`LEASE`] that engine
//! renews ([`renew`]). Every write a holder makes ([`start`], [`finish`], [`unassign`]) is fenced
//! by its token, so an engine whose lease lapsed can never record over the one that took the slot
//! over. An engine that dies stops renewing, and the next takes its slots once their leases lapse
//! ([`adopt`]); one that stops cleanly lets its leases lapse at once ([`release`]), so the next
//! takes them as it starts. Unlike a workflow step, a slot taken over is never tried again from
//! the start: a `running` batch's worker lives in a container of its own, which outlives the
//! engine, so the engine that adopts it finds that worker again rather than starting another.
//!
//! A slot is given back when its batch ends ([`finish`]), is paused or cancelled before a worker
//! started it ([`change`], [`cancel`]), or is taken back because the pool shrank ([`unassign`]).
//!
//! # History
//!
//! Every change to a batch is written to its history (`work_batch_events`) in the transaction
//! that makes it, with who made it: its submission, every change a person, Elysia, or its Project
//! Manager made while it waited, and each move the pool and its worker made ([`EventKind`]).

use chrono::{DateTime, TimeDelta, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::database::schema::{work_batch_events, work_batches};
use crate::models::project_manager;
use crate::models::user::{PROJECT_MANAGER_USER_ID, SYSTEM_USER_ID};

/// How many batches one page of a project's batches holds at most.
pub const MAX_BATCH_PAGE: i64 = 200;

/// How long a slot's claim lasts unless renewed. Its engine renews it four times as often, so a
/// slow database never lets a held slot lapse; short enough that the slots of an engine that died
/// are taken over within a minute.
pub const LEASE: TimeDelta = TimeDelta::seconds(60);

/// How far ahead a batch may be scheduled. Further than a year is a plan, not a batch.
pub const SCHEDULE_HORIZON: TimeDelta = TimeDelta::days(365);

/// The longest cancellation reason and result, matching `work_batches.result`.
pub const RESULT_MAX_CHARACTERS: usize = 20_000;

/// Serializes every pass that hands out slots, in whichever process makes it, so two never
/// count the same free slot.
const ASSIGN_LOCK: &str = "work_batches.assign";

/// When a batch should run.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::WorkBatchPriority"]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// Before every normal batch.
    Asap,
    Normal,
    /// At its `scheduled_for`, before anything else once it is due.
    Scheduled,
}

/// Where a batch stands.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::WorkBatchState"]
#[serde(rename_all = "snake_case")]
pub enum BatchState {
    /// Waiting for a slot in the worker pool.
    Queued,
    /// Held back by a person, Elysia, or its Project Manager; it waits until resumed.
    Paused,
    /// The pool gave it a slot; a worker has not started on it yet.
    Assigned,
    /// A worker has it.
    Running,
    Succeeded,
    Failed,
    /// Withdrawn before a worker started on it.
    Cancelled,
}

impl BatchState {
    /// Whether a batch in this state waits for a worker, so it may still be paused or cancelled.
    pub const fn is_waiting(self) -> bool {
        matches!(self, Self::Queued | Self::Paused | Self::Assigned)
    }
}

/// One repository a batch needs, as a coding session names one: a Git URL and the branch its
/// work starts from, the remote's default when `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Repository {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
}

/// A stored batch, without the claim an engine holds its slot under ([`Slot`]).
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = work_batches, check_for_backend(diesel::pg::Pg))]
pub struct WorkBatch {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub goal: String,
    /// [`Repository`]s, as stored.
    pub repositories: Value,
    pub priority: Priority,
    pub scheduled_for: Option<DateTime<Utc>>,
    pub state: BatchState,
    pub result: Option<String>,
    pub turn_id: Option<Uuid>,
    /// Ids of timeline records, as stored.
    pub source_record_ids: Value,
    /// Ids of action items, as stored.
    pub action_item_ids: Value,
    pub timeline_record_id: Option<Uuid>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    /// Set while someone asked for it to run next, ahead of everything else.
    pub pinned_at: Option<DateTime<Utc>>,
    /// When the pool gave it a slot.
    pub assigned_at: Option<DateTime<Utc>>,
    /// Until when its slot's holder owns the slot, unless it renews.
    pub lease_expires_at: Option<DateTime<Utc>>,
}

impl WorkBatch {
    /// The repositories it needs. A row this build cannot read lists none, logged, rather than
    /// hiding the batch.
    pub fn repositories(&self) -> Vec<Repository> {
        serde_json::from_value(self.repositories.clone()).unwrap_or_else(|error| {
            tracing::event!(
                name: "work_batch.repositories.unreadable",
                tracing::Level::WARN,
                work_batch.id = %self.id,
                error.message = %error,
                "a batch's stored repositories do not read; listing none",
            );
            Vec::new()
        })
    }

    /// The timeline records that called for it.
    pub fn source_records(&self) -> Vec<Uuid> {
        serde_json::from_value(self.source_record_ids.clone()).unwrap_or_default()
    }

    /// The action items it works on.
    pub fn action_items(&self) -> Vec<Uuid> {
        serde_json::from_value(self.action_item_ids.clone()).unwrap_or_default()
    }

    /// Whether it may be given a slot at `now`: queued, and due when scheduled.
    pub fn is_due(&self, now: DateTime<Utc>) -> bool {
        self.state == BatchState::Queued
            && (self.priority != Priority::Scheduled
                || self.scheduled_for.is_some_and(|due| due <= now))
    }
}

/// A batch holding a slot, and the token fencing every write its holder makes.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = work_batches, check_for_backend(diesel::pg::Pg))]
pub struct Slot {
    #[diesel(embed)]
    pub batch: WorkBatch,
    #[diesel(select_expression = work_batches::claim_token.assume_not_null())]
    pub token: Uuid,
}

/// What a history entry records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// Its Project Manager submitted it. No data.
    Submitted,
    /// Someone asked for it to run next. No data.
    Pinned,
    /// It went back to its place in the queue. No data.
    Unpinned,
    /// Its priority changed: `{ before, after }`, each `{ priority, scheduledFor }`.
    Reprioritized,
    /// It was held back: `{ from }`, the state it left.
    Paused,
    /// It waits for a slot again. No data.
    Resumed,
    /// It was withdrawn: `{ reason }`.
    Cancelled,
    /// The pool gave it a slot. No data.
    Assigned,
    /// The pool took its slot back, since the pool shrank. No data.
    Unassigned,
    /// A worker started on it. No data.
    Started,
    /// Its worker ended it: `{ succeeded }`.
    Finished,
}

impl EventKind {
    /// The kind as `work_batch_events.kind` stores it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::Pinned => "pinned",
            Self::Unpinned => "unpinned",
            Self::Reprioritized => "reprioritized",
            Self::Paused => "paused",
            Self::Resumed => "resumed",
            Self::Cancelled => "cancelled",
            Self::Assigned => "assigned",
            Self::Unassigned => "unassigned",
            Self::Started => "started",
            Self::Finished => "finished",
        }
    }
}

/// One entry of a batch's history.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = work_batch_events, check_for_backend(diesel::pg::Pg))]
pub struct BatchEvent {
    pub id: Uuid,
    /// An [`EventKind`], as stored.
    pub kind: String,
    pub data: Value,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = work_batch_events)]
struct NewBatchEvent<'a> {
    id: Uuid,
    work_batch_id: Uuid,
    kind: &'a str,
    data: Value,
    created_by: Uuid,
}

/// Writes one entry of batch `batch_id`'s history, by `by`.
async fn record(
    connection: &mut AsyncPgConnection,
    batch_id: Uuid,
    kind: EventKind,
    data: Value,
    by: Uuid,
) -> QueryResult<()> {
    diesel::insert_into(work_batch_events::table)
        .values(NewBatchEvent {
            id: Uuid::now_v7(),
            work_batch_id: batch_id,
            kind: kind.as_str(),
            data,
            created_by: by,
        })
        .execute(connection)
        .await
        .map(drop)
}

/// A batch to submit.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = work_batches)]
pub struct NewWorkBatch {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub goal: String,
    pub repositories: Value,
    pub priority: Priority,
    pub scheduled_for: Option<DateTime<Utc>>,
    pub turn_id: Option<Uuid>,
    pub source_record_ids: Value,
    pub action_item_ids: Value,
    pub created_by: Uuid,
}

/// The rule every batch's schedule is held to at `now`: a scheduled time is given exactly for a
/// scheduled batch, in the future, and within a year. Answers what to change.
///
/// # Errors
/// Names the rule `priority` and `scheduled_for` break.
pub fn check_schedule(
    priority: Priority,
    scheduled_for: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<(), String> {
    match (priority, scheduled_for) {
        (Priority::Scheduled, None) => Err(
            "a scheduled batch needs scheduledFor, the time it is due, with its offset".to_owned(),
        ),
        (Priority::Scheduled, Some(at)) if at <= now => {
            Err("scheduledFor is not in the future; make it asap or normal instead".to_owned())
        }
        (Priority::Scheduled, Some(at)) if at > now + SCHEDULE_HORIZON => {
            Err("scheduledFor is more than a year away".to_owned())
        }
        (Priority::Asap | Priority::Normal, Some(_)) => {
            Err("scheduledFor is only for a scheduled batch".to_owned())
        }
        _ => Ok(()),
    }
}

/// Stores a new batch, queued, and begins its history.
///
/// # Errors
/// Propagates any database error, including a field outside its column's checks.
pub async fn submit(
    connection: &mut AsyncPgConnection,
    batch: NewWorkBatch,
) -> QueryResult<WorkBatch> {
    connection
        .transaction(async |connection| {
            let by = batch.created_by;
            let submitted = diesel::insert_into(work_batches::table)
                .values(batch)
                .returning(WorkBatch::as_returning())
                .get_result(connection)
                .await?;
            record(
                connection,
                submitted.id,
                EventKind::Submitted,
                json!({}),
                by,
            )
            .await?;
            Ok(submitted)
        })
        .await
}

/// Records the timeline record batch `id` put on its project's timeline.
///
/// # Errors
/// Propagates any database error.
pub async fn set_timeline_record(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    record_id: Uuid,
) -> QueryResult<()> {
    diesel::update(work_batches::table.find(id))
        .set(work_batches::timeline_record_id.eq(record_id))
        .execute(connection)
        .await
        .map(drop)
}

/// Batch `id`.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] for an unknown batch; any other database error.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<WorkBatch> {
    work_batches::table
        .find(id)
        .select(WorkBatch::as_select())
        .first(connection)
        .await
}

/// The batches among `ids`, in no particular order.
///
/// # Errors
/// Propagates any database error.
pub async fn find_many(
    connection: &mut AsyncPgConnection,
    ids: &[Uuid],
) -> QueryResult<Vec<WorkBatch>> {
    work_batches::table
        .filter(work_batches::id.eq_any(ids))
        .select(WorkBatch::as_select())
        .load(connection)
        .await
}

/// `project_id`'s latest batches in `states`, or every state when it is empty, newest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list_for_project(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    states: &[BatchState],
    limit: i64,
) -> QueryResult<Vec<WorkBatch>> {
    let mut query = work_batches::table
        .filter(work_batches::project_id.eq(project_id))
        .select(WorkBatch::as_select())
        .order(work_batches::id.desc())
        .limit(limit.clamp(1, MAX_BATCH_PAGE))
        .into_boxed();
    if !states.is_empty() {
        query = query.filter(work_batches::state.eq_any(states.to_vec()));
    }
    query.load(connection).await
}

/// Every batch in `states`, oldest first, at most `limit`.
///
/// # Errors
/// Propagates any database error.
pub async fn in_states(
    connection: &mut AsyncPgConnection,
    states: &[BatchState],
    limit: i64,
) -> QueryResult<Vec<WorkBatch>> {
    work_batches::table
        .filter(work_batches::state.eq_any(states.to_vec()))
        .select(WorkBatch::as_select())
        .order(work_batches::id)
        .limit(limit)
        .load(connection)
        .await
}

/// Every batch holding a slot, in the order the slots were handed out.
///
/// # Errors
/// Propagates any database error.
pub async fn in_slots(connection: &mut AsyncPgConnection) -> QueryResult<Vec<WorkBatch>> {
    work_batches::table
        .filter(work_batches::claim_token.is_not_null())
        .select(WorkBatch::as_select())
        .order((work_batches::assigned_at, work_batches::id))
        .load(connection)
        .await
}

/// Batch `id`'s history, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn history(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Vec<BatchEvent>> {
    work_batch_events::table
        .filter(work_batch_events::work_batch_id.eq(id))
        .select(BatchEvent::as_select())
        .order(work_batch_events::id)
        .load(connection)
        .await
}

/// Why a change to a batch was refused.
#[derive(Debug, thiserror::Error)]
pub enum ChangeError {
    /// No such batch, or none in the project the caller is limited to.
    #[error("no such batch")]
    NotFound,
    /// The batch's state does not allow it; the message says what does.
    #[error("{0}")]
    Refused(String),
    /// What was asked for breaks a rule whatever the batch's state, such as a scheduled time in
    /// the past; the message says what to change.
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Database(diesel::result::Error),
}

impl From<diesel::result::Error> for ChangeError {
    fn from(error: diesel::result::Error) -> Self {
        match error {
            diesel::result::Error::NotFound => Self::NotFound,
            other => Self::Database(other),
        }
    }
}

/// A change a person, Elysia, or its Project Manager makes to a batch while it waits. Each
/// field left `None` stays as it is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Changes {
    /// A new priority, with the time a scheduled one is due.
    pub priority: Option<(Priority, Option<DateTime<Utc>>)>,
    /// True holds it back; false lets it wait for a slot again.
    pub paused: Option<bool>,
    /// True runs it next, ahead of everything else; false puts it back in its place.
    pub run_next: Option<bool>,
}

impl Changes {
    /// Whether it changes nothing.
    pub fn is_empty(&self) -> bool {
        self.priority.is_none() && self.paused.is_none() && self.run_next.is_none()
    }
}

/// Applies `changes` to batch `id` at `now`, as `by`, in one transaction under the batch's lock:
/// its priority first, then pausing or resuming, then running it next. Each change that moves
/// the batch is written to its history; one that leaves it as it was writes nothing. Answers the
/// batch and whether anything changed.
///
/// # Errors
/// [`ChangeError::NotFound`] for an unknown batch; [`ChangeError::Refused`] naming why, when its
/// state does not allow a change; [`ChangeError::Invalid`] for a schedule the rules refuse, or
/// changes that contradict each other; any other database error.
pub async fn change(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    changes: Changes,
    by: Uuid,
    now: DateTime<Utc>,
) -> Result<(WorkBatch, bool), ChangeError> {
    if changes.paused == Some(true) && changes.run_next == Some(true) {
        return Err(ChangeError::Invalid(
            "a batch cannot be paused and run next at once".to_owned(),
        ));
    }
    connection
        .transaction(async |connection| {
            let mut batch = lock(connection, id).await?;
            let mut moved = false;
            if let Some((priority, scheduled_for)) = changes.priority {
                moved |=
                    reprioritize(connection, &mut batch, priority, scheduled_for, by, now).await?;
            }
            if let Some(paused) = changes.paused {
                moved |= set_paused(connection, &mut batch, paused, by).await?;
            }
            if let Some(run_next) = changes.run_next {
                moved |= set_run_next(connection, &mut batch, run_next, by, now).await?;
            }
            Ok((batch, moved))
        })
        .await
}

/// Locks batch `id` for a change.
async fn lock(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<WorkBatch> {
    work_batches::table
        .find(id)
        .select(WorkBatch::as_select())
        .for_update()
        .first(connection)
        .await
}

async fn reprioritize(
    connection: &mut AsyncPgConnection,
    batch: &mut WorkBatch,
    priority: Priority,
    scheduled_for: Option<DateTime<Utc>>,
    by: Uuid,
    now: DateTime<Utc>,
) -> Result<bool, ChangeError> {
    if !matches!(batch.state, BatchState::Queued | BatchState::Paused) {
        return Err(ChangeError::Refused(format!(
            "batch {} is {}; only a queued or paused batch changes priority",
            batch.id,
            state_words(batch.state)
        )));
    }
    if batch.priority == priority && batch.scheduled_for == scheduled_for {
        return Ok(false);
    }
    check_schedule(priority, scheduled_for, now).map_err(ChangeError::Invalid)?;
    // A batch scheduled for later is not due, so it cannot stay first in line.
    let pinned_at = batch
        .pinned_at
        .filter(|_pinned| priority != Priority::Scheduled);
    let before = json!({ "priority": batch.priority, "scheduledFor": batch.scheduled_for });
    *batch = diesel::update(work_batches::table.find(batch.id))
        .set((
            work_batches::priority.eq(priority),
            work_batches::scheduled_for.eq(scheduled_for),
            work_batches::pinned_at.eq(pinned_at),
        ))
        .returning(WorkBatch::as_returning())
        .get_result(connection)
        .await?;
    let after = json!({ "priority": priority, "scheduledFor": scheduled_for });
    record(
        connection,
        batch.id,
        EventKind::Reprioritized,
        json!({ "before": before, "after": after }),
        by,
    )
    .await?;
    Ok(true)
}

async fn set_paused(
    connection: &mut AsyncPgConnection,
    batch: &mut WorkBatch,
    paused: bool,
    by: Uuid,
) -> Result<bool, ChangeError> {
    let from = batch.state;
    let (to, kind, data) = match (paused, from) {
        (true, BatchState::Paused) | (false, BatchState::Queued | BatchState::Assigned) => {
            return Ok(false);
        }
        (true, BatchState::Queued | BatchState::Assigned) => (
            BatchState::Paused,
            EventKind::Paused,
            json!({ "from": from }),
        ),
        (false, BatchState::Paused) => (BatchState::Queued, EventKind::Resumed, json!({})),
        (true, _) => {
            return Err(ChangeError::Refused(format!(
                "batch {} is {}; only a batch no worker has started on can be paused",
                batch.id,
                state_words(from)
            )));
        }
        (false, _) => {
            return Err(ChangeError::Refused(format!(
                "batch {} is {}; only a paused batch can be resumed",
                batch.id,
                state_words(from)
            )));
        }
    };
    // Waiting again, it holds no slot and keeps no place at the front: it waits in its place.
    *batch = diesel::update(work_batches::table.find(batch.id))
        .set((
            work_batches::state.eq(to),
            work_batches::pinned_at.eq(None::<DateTime<Utc>>),
            work_batches::assigned_at.eq(None::<DateTime<Utc>>),
            work_batches::claim_token.eq(None::<Uuid>),
            work_batches::lease_expires_at.eq(None::<DateTime<Utc>>),
        ))
        .returning(WorkBatch::as_returning())
        .get_result(connection)
        .await?;
    record(connection, batch.id, kind, data, by).await?;
    Ok(true)
}

async fn set_run_next(
    connection: &mut AsyncPgConnection,
    batch: &mut WorkBatch,
    run_next: bool,
    by: Uuid,
    now: DateTime<Utc>,
) -> Result<bool, ChangeError> {
    if run_next == batch.pinned_at.is_some() {
        return Ok(false);
    }
    if run_next && !batch.is_due(now) {
        let reason = if batch.state == BatchState::Queued {
            format!(
                "batch {} is scheduled for later; make it asap to run it now",
                batch.id
            )
        } else {
            format!(
                "batch {} is {}; only a queued batch can run next",
                batch.id,
                state_words(batch.state)
            )
        };
        return Err(ChangeError::Refused(reason));
    }
    let (pinned_at, kind) = if run_next {
        (Some(now), EventKind::Pinned)
    } else {
        (None, EventKind::Unpinned)
    };
    *batch = diesel::update(work_batches::table.find(batch.id))
        .set(work_batches::pinned_at.eq(pinned_at))
        .returning(WorkBatch::as_returning())
        .get_result(connection)
        .await?;
    record(connection, batch.id, kind, json!({}), by).await?;
    Ok(true)
}

/// Withdraws batch `id`, saying `reason`, as `by`, while no worker has started on it, giving
/// back the slot it may hold. With `project_id`, only a batch of that project, as its Project
/// Manager may withdraw its own. Anyone but its Project Manager wakes it with the cancellation in
/// the same transaction, so it learns its work was withdrawn and why.
///
/// # Errors
/// [`ChangeError::NotFound`] for an unknown batch or one of another project;
/// [`ChangeError::Refused`] once a worker started on it; any other database error, including a
/// reason outside 1 to 20,000 characters.
pub async fn cancel(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    project_id: Option<Uuid>,
    reason: &str,
    by: Uuid,
) -> Result<WorkBatch, ChangeError> {
    connection
        .transaction(async |connection| {
            let batch = lock(connection, id).await?;
            if project_id.is_some_and(|project_id| project_id != batch.project_id) {
                return Err(ChangeError::NotFound);
            }
            if !batch.state.is_waiting() {
                return Err(ChangeError::Refused(format!(
                    "batch {id} is {}; only a batch no worker has started on can be cancelled",
                    state_words(batch.state)
                )));
            }
            let cancelled = diesel::update(work_batches::table.find(id))
                .set((
                    work_batches::state.eq(BatchState::Cancelled),
                    work_batches::result.eq(reason),
                    work_batches::finished_at.eq(diesel::dsl::now),
                    work_batches::pinned_at.eq(None::<DateTime<Utc>>),
                    work_batches::claim_token.eq(None::<Uuid>),
                    work_batches::lease_expires_at.eq(None::<DateTime<Utc>>),
                ))
                .returning(WorkBatch::as_returning())
                .get_result(connection)
                .await?;
            record(
                connection,
                id,
                EventKind::Cancelled,
                json!({ "reason": reason }),
                by,
            )
            .await?;
            if by != PROJECT_MANAGER_USER_ID {
                project_manager::wake_for_batch(connection, cancelled.project_id, id, by).await?;
            }
            Ok(cancelled)
        })
        .await
}

/// Gives a slot to each of `ids`, in that order, while fewer than `size` slots are in use, at
/// `at`: each still queued and not locked by someone changing it becomes `assigned`, claimed
/// under a token of its own and a [`LEASE`], and leaves its place at the front. Answers the slots
/// handed out.
///
/// Every pass holds one transaction-scoped lock, so two never count the same free slot; it skips
/// a batch someone is changing rather than waiting on them.
///
/// # Errors
/// Propagates any database error.
pub async fn assign(
    connection: &mut AsyncPgConnection,
    ids: &[Uuid],
    size: usize,
    at: DateTime<Utc>,
) -> QueryResult<Vec<Slot>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    connection
        .transaction(async |connection| {
            diesel::sql_query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
                .bind::<diesel::sql_types::Text, _>(ASSIGN_LOCK)
                .execute(connection)
                .await?;
            let in_use: i64 = work_batches::table
                .filter(work_batches::claim_token.is_not_null())
                .count()
                .get_result(connection)
                .await?;
            let free = size.saturating_sub(usize::try_from(in_use).unwrap_or(usize::MAX));
            if free == 0 {
                return Ok(Vec::new());
            }
            let available: Vec<Uuid> = work_batches::table
                .filter(work_batches::id.eq_any(ids))
                .filter(work_batches::state.eq(BatchState::Queued))
                .select(work_batches::id)
                .for_update()
                .skip_locked()
                .load(connection)
                .await?;
            let chosen: Vec<Uuid> = ids
                .iter()
                .filter(|id| available.contains(id))
                .take(free)
                .copied()
                .collect();
            if chosen.is_empty() {
                return Ok(Vec::new());
            }
            let mut slots: Vec<Slot> =
                diesel::update(work_batches::table.filter(work_batches::id.eq_any(&chosen)))
                    .set((
                        work_batches::state.eq(BatchState::Assigned),
                        work_batches::assigned_at.eq(at),
                        work_batches::pinned_at.eq(None::<DateTime<Utc>>),
                        // Each row its own, so one token fences one slot.
                        work_batches::claim_token.eq(diesel::dsl::sql::<
                            diesel::sql_types::Nullable<diesel::sql_types::Uuid>,
                        >("gen_random_uuid()")),
                        work_batches::lease_expires_at.eq(at + LEASE),
                    ))
                    .returning(Slot::as_returning())
                    .get_results(connection)
                    .await?;
            slots.sort_by_key(|slot| chosen.iter().position(|id| *id == slot.batch.id));
            for slot in &slots {
                record(
                    connection,
                    slot.batch.id,
                    EventKind::Assigned,
                    json!({}),
                    SYSTEM_USER_ID,
                )
                .await?;
            }
            Ok(slots)
        })
        .await
}

/// Extends the leases of `held` slots, each a batch and its token, from `at`. Answers the
/// batches whose slot is still held: one missing was paused, cancelled, or taken over since, so
/// its holder lets it go.
///
/// # Errors
/// Propagates any database error.
pub async fn renew(
    connection: &mut AsyncPgConnection,
    held: &[(Uuid, Uuid)],
    at: DateTime<Utc>,
) -> QueryResult<Vec<Uuid>> {
    if held.is_empty() {
        return Ok(Vec::new());
    }
    let tokens: Vec<Uuid> = held.iter().map(|(_batch, token)| *token).collect();
    diesel::update(work_batches::table.filter(work_batches::claim_token.eq_any(tokens)))
        .set(work_batches::lease_expires_at.eq(at + LEASE))
        .returning(work_batches::id)
        .get_results(connection)
        .await
}

/// Takes over every slot whose lease lapsed by `at`, each under a new token and [`LEASE`]: the
/// slots of an engine that died, once their leases run out, and those an engine released as it
/// stopped. The batches keep their state, since a worker's container outlives its engine.
///
/// # Errors
/// Propagates any database error.
pub async fn adopt(
    connection: &mut AsyncPgConnection,
    at: DateTime<Utc>,
) -> QueryResult<Vec<Slot>> {
    connection
        .transaction(async |connection| {
            let lapsed: Vec<Uuid> = work_batches::table
                .filter(work_batches::claim_token.is_not_null())
                .filter(work_batches::lease_expires_at.le(at))
                .select(work_batches::id)
                .for_update()
                .skip_locked()
                .load(connection)
                .await?;
            if lapsed.is_empty() {
                return Ok(Vec::new());
            }
            diesel::update(work_batches::table.filter(work_batches::id.eq_any(&lapsed)))
                .set((
                    work_batches::claim_token.eq(diesel::dsl::sql::<
                        diesel::sql_types::Nullable<diesel::sql_types::Uuid>,
                    >("gen_random_uuid()")),
                    work_batches::lease_expires_at.eq(at + LEASE),
                ))
                .returning(Slot::as_returning())
                .get_results(connection)
                .await
        })
        .await
}

/// Lets the leases of `held` slots lapse at `at`, for an engine that is stopping: the next
/// engine takes them over as it starts rather than a lease later.
///
/// # Errors
/// Propagates any database error.
pub async fn release(
    connection: &mut AsyncPgConnection,
    held: &[(Uuid, Uuid)],
    at: DateTime<Utc>,
) -> QueryResult<usize> {
    if held.is_empty() {
        return Ok(0);
    }
    let tokens: Vec<Uuid> = held.iter().map(|(_batch, token)| *token).collect();
    diesel::update(work_batches::table.filter(work_batches::claim_token.eq_any(tokens)))
        .set(work_batches::lease_expires_at.eq(at))
        .execute(connection)
        .await
}

/// Takes `slot` back from its batch while no worker has started on it, returning the batch to
/// the queue in its place: the pool shrank below the slots in use. `None` when the slot was lost
/// or a worker started on it.
///
/// # Errors
/// Propagates any database error.
pub async fn unassign(
    connection: &mut AsyncPgConnection,
    slot: &Slot,
) -> QueryResult<Option<WorkBatch>> {
    connection
        .transaction(async |connection| {
            let Some(queued) = diesel::update(
                work_batches::table
                    .find(slot.batch.id)
                    .filter(work_batches::claim_token.eq(slot.token))
                    .filter(work_batches::state.eq(BatchState::Assigned)),
            )
            .set((
                work_batches::state.eq(BatchState::Queued),
                work_batches::assigned_at.eq(None::<DateTime<Utc>>),
                work_batches::claim_token.eq(None::<Uuid>),
                work_batches::lease_expires_at.eq(None::<DateTime<Utc>>),
            ))
            .returning(WorkBatch::as_returning())
            .get_result(connection)
            .await
            .optional()?
            else {
                return Ok(None);
            };
            record(
                connection,
                queued.id,
                EventKind::Unassigned,
                json!({}),
                SYSTEM_USER_ID,
            )
            .await?;
            Ok(Some(queued))
        })
        .await
}

/// A worker starts on `slot`'s batch. `None` when the slot was lost, or the batch is no longer
/// waiting for a worker.
///
/// # Errors
/// Propagates any database error.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the pool's workers (#85) start their batches through it"
    )
)]
pub async fn start(
    connection: &mut AsyncPgConnection,
    slot: &Slot,
) -> QueryResult<Option<WorkBatch>> {
    connection
        .transaction(async |connection| {
            let Some(started) = diesel::update(
                work_batches::table
                    .find(slot.batch.id)
                    .filter(work_batches::claim_token.eq(slot.token))
                    .filter(work_batches::state.eq(BatchState::Assigned)),
            )
            .set((
                work_batches::state.eq(BatchState::Running),
                work_batches::started_at.eq(diesel::dsl::now),
            ))
            .returning(WorkBatch::as_returning())
            .get_result(connection)
            .await
            .optional()?
            else {
                return Ok(None);
            };
            record(
                connection,
                started.id,
                EventKind::Started,
                json!({}),
                SYSTEM_USER_ID,
            )
            .await?;
            Ok(Some(started))
        })
        .await
}

/// Ends `slot`'s running batch, succeeded or not, with what its worker reported, giving its slot
/// back, and wakes its project's Project Manager with the result in the same transaction, as
/// `by`. `None` when the slot was lost or the batch is not running. Once it commits, the caller
/// sends `Signal::WorkerPool` and `Signal::ProjectManagers` and publishes `workBatch.changed` and
/// `projectManager.changed`, so the pool fills the slot and the Project Manager looks at once.
///
/// # Errors
/// Propagates any database error, including a result outside 1 to 20,000 characters.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the pool's workers (#85) report their batches through it"
    )
)]
pub async fn finish(
    connection: &mut AsyncPgConnection,
    slot: &Slot,
    succeeded: bool,
    result: &str,
    by: Uuid,
) -> QueryResult<Option<WorkBatch>> {
    let state = if succeeded {
        BatchState::Succeeded
    } else {
        BatchState::Failed
    };
    connection
        .transaction(async |connection| {
            let Some(finished) = diesel::update(
                work_batches::table
                    .find(slot.batch.id)
                    .filter(work_batches::claim_token.eq(slot.token))
                    .filter(work_batches::state.eq(BatchState::Running)),
            )
            .set((
                work_batches::state.eq(state),
                work_batches::result.eq(result),
                work_batches::finished_at.eq(diesel::dsl::now),
                work_batches::claim_token.eq(None::<Uuid>),
                work_batches::lease_expires_at.eq(None::<DateTime<Utc>>),
            ))
            .returning(WorkBatch::as_returning())
            .get_result(connection)
            .await
            .optional()?
            else {
                return Ok(None);
            };
            record(
                connection,
                finished.id,
                EventKind::Finished,
                json!({ "succeeded": succeeded }),
                by,
            )
            .await?;
            project_manager::wake_for_batch(connection, finished.project_id, finished.id, by)
                .await?;
            Ok(Some(finished))
        })
        .await
}

/// A state as a refusal says it.
const fn state_words(state: BatchState) -> &'static str {
    match state {
        BatchState::Queued => "queued",
        BatchState::Paused => "paused",
        BatchState::Assigned => "waiting for its worker",
        BatchState::Running => "running",
        BatchState::Succeeded => "finished",
        BatchState::Failed => "finished, failed",
        BatchState::Cancelled => "cancelled",
    }
}

#[cfg(test)]
mod tests;
