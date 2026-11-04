// Copyright © 2026 Jalapeno Labs

//! Work batches: what a Project Manager submits for the engine's worker pool. See
//! `docs/engine.md`, Workers and the pool.
//!
//! A batch is a goal a worker can work from alone, the repositories it needs, a priority (ASAP,
//! normal, or a scheduled time), and its provenance: the Project Manager's turn that submitted
//! it, the timeline records that called for it, and the action items it works on.
//!
//! A batch moves one way: `queued` until a worker takes it ([`start`]), `running`, then
//! `succeeded` or `failed` ([`finish`]), which wakes its project's Project Manager in the same
//! transaction. A queued batch may be `cancelled` instead ([`cancel`]).
//!
//! The scheduler (#84) hands out queued batches over `work_batches_queue`, which orders them as
//! it does: due scheduled batches first, then ASAP, then normal, oldest first within each.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::database::schema::work_batches;
use crate::models::project_manager;

/// How many batches one page of a project's batches holds at most.
pub const MAX_BATCH_PAGE: i64 = 200;

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
    /// A worker has it.
    Running,
    Succeeded,
    Failed,
    /// Withdrawn before a worker took it.
    Cancelled,
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

/// A stored batch.
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

/// Stores a new batch, queued.
///
/// # Errors
/// Propagates any database error, including a field outside its column's checks.
pub async fn submit(
    connection: &mut AsyncPgConnection,
    batch: NewWorkBatch,
) -> QueryResult<WorkBatch> {
    diesel::insert_into(work_batches::table)
        .values(batch)
        .returning(WorkBatch::as_returning())
        .get_result(connection)
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

/// Withdraws `project_id`'s batch `id` while it is still queued, saying `reason`. `None` when
/// the project has no such queued batch.
///
/// # Errors
/// Propagates any database error.
pub async fn cancel(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    id: Uuid,
    reason: &str,
) -> QueryResult<Option<WorkBatch>> {
    diesel::update(
        work_batches::table
            .find(id)
            .filter(work_batches::project_id.eq(project_id))
            .filter(work_batches::state.eq(BatchState::Queued)),
    )
    .set((
        work_batches::state.eq(BatchState::Cancelled),
        work_batches::result.eq(reason),
        work_batches::finished_at.eq(diesel::dsl::now),
    ))
    .returning(WorkBatch::as_returning())
    .get_result(connection)
    .await
    .optional()
}

/// Hands queued batch `id` to a worker. `None` when it is no longer queued.
///
/// # Errors
/// Propagates any database error.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the worker pool's scheduler (#84) takes batches through it"
    )
)]
pub async fn start(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Option<WorkBatch>> {
    diesel::update(
        work_batches::table
            .find(id)
            .filter(work_batches::state.eq(BatchState::Queued)),
    )
    .set((
        work_batches::state.eq(BatchState::Running),
        work_batches::started_at.eq(diesel::dsl::now),
    ))
    .returning(WorkBatch::as_returning())
    .get_result(connection)
    .await
    .optional()
}

/// Ends running batch `id`, succeeded or not, with what its worker reported, and wakes its
/// project's Project Manager with the result in the same transaction, as `by`. `None` when it
/// is not running. Once it commits, the caller sends `Signal::ProjectManagers` and publishes
/// `projectManager.changed`, so the Project Manager looks at once rather than at its next tick.
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
    id: Uuid,
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
                    .find(id)
                    .filter(work_batches::state.eq(BatchState::Running)),
            )
            .set((
                work_batches::state.eq(state),
                work_batches::result.eq(result),
                work_batches::finished_at.eq(diesel::dsl::now),
            ))
            .returning(WorkBatch::as_returning())
            .get_result(connection)
            .await
            .optional()?
            else {
                return Ok(None);
            };
            project_manager::wake_for_batch(connection, finished.project_id, finished.id, by)
                .await?;
            Ok(Some(finished))
        })
        .await
}
