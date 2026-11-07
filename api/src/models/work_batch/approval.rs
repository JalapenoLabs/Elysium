// Copyright © 2026 Jalapeno Labs

//! A person's decision on a proposed batch: approve it as it is, change its title, goal,
//! repositories, or priority and approve it, or reject it with a reason.
//!
//! Approving moves it to `queued`, where the pool may give it a slot; rejecting ends it
//! `rejected`, with the reason as its result, and wakes its Project Manager with it in the same
//! transaction, as a cancellation does, so it plans again knowing why. Both write the batch's
//! history as the person who decided, and bring the project's approval item in line
//! ([`super::approval_item`]).
//!
//! Only a person decides. These functions take the person's id; the one way in,
//! `crate::worker_pool::actions`, takes a `MemberUser`, which no machine user can be, so no tool
//! an agent holds reaches them.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::approval_item::{self, Cause};
use super::{
    BatchState, ChangeError, EventKind, Priority, Repository, WorkBatch, Written, lock, record,
    state_words,
};
use crate::database::schema::work_batches;
use crate::models::project_manager;

/// What a person changes as they approve a proposed batch. Each field left `None` stays as the
/// Project Manager proposed it. The values are already checked and tidied by the rules every batch
/// is held to (`crate::project_managers::batches::check_edits`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Edits {
    pub title: Option<String>,
    pub goal: Option<String>,
    pub repositories: Option<Vec<Repository>>,
    /// A new priority, with the time a scheduled one is due.
    pub priority: Option<(Priority, Option<DateTime<Utc>>)>,
}

/// Approves proposed batch `id` as person `by`, first applying `edits`, in one transaction under
/// the batch's lock: an `edited` entry holds what changed, when anything did, then `approved`.
///
/// # Errors
/// [`ChangeError::NotFound`] for an unknown batch; [`ChangeError::Refused`] once it is no longer
/// proposed; any other database error.
pub async fn approve(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    edits: Edits,
    by: Uuid,
    now: DateTime<Utc>,
) -> Result<Written, ChangeError> {
    connection
        .transaction(async |connection| {
            let proposed = lock_proposed(connection, id).await?;
            let (before, after) = changed_fields(&proposed, &edits);
            let repositories = edits.repositories.map(|repositories| json!(repositories));
            let (priority, scheduled_for) = edits
                .priority
                .unwrap_or((proposed.priority, proposed.scheduled_for));
            let batch = diesel::update(work_batches::table.find(id))
                .set((
                    work_batches::state.eq(BatchState::Queued),
                    work_batches::title.eq(edits.title.unwrap_or(proposed.title)),
                    work_batches::goal.eq(edits.goal.unwrap_or(proposed.goal)),
                    work_batches::repositories.eq(repositories.unwrap_or(proposed.repositories)),
                    work_batches::priority.eq(priority),
                    work_batches::scheduled_for.eq(scheduled_for),
                ))
                .returning(WorkBatch::as_returning())
                .get_result(connection)
                .await?;
            if !after.is_empty() {
                let data = json!({ "before": before, "after": after });
                record(connection, id, EventKind::Edited, data, by).await?;
            }
            record(connection, id, EventKind::Approved, json!({}), by).await?;
            settle(connection, batch, by, now).await
        })
        .await
}

/// Rejects proposed batch `id` as person `by`, saying `reason`, which becomes its result and wakes
/// its Project Manager with it, in one transaction under the batch's lock.
///
/// # Errors
/// [`ChangeError::NotFound`] for an unknown batch; [`ChangeError::Refused`] once it is no longer
/// proposed; any other database error, including a reason outside 1 to 20,000 characters.
pub async fn reject(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    reason: &str,
    by: Uuid,
    now: DateTime<Utc>,
) -> Result<Written, ChangeError> {
    connection
        .transaction(async |connection| {
            lock_proposed(connection, id).await?;
            let batch = diesel::update(work_batches::table.find(id))
                .set((
                    work_batches::state.eq(BatchState::Rejected),
                    work_batches::result.eq(reason),
                    work_batches::finished_at.eq(now),
                ))
                .returning(WorkBatch::as_returning())
                .get_result(connection)
                .await?;
            record(
                connection,
                id,
                EventKind::Rejected,
                json!({ "reason": reason }),
                by,
            )
            .await?;
            project_manager::wake_for_batch(connection, batch.project_id, id, by).await?;
            settle(connection, batch, by, now).await
        })
        .await
}

/// Batch `id`, locked, while it is proposed.
async fn lock_proposed(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> Result<WorkBatch, ChangeError> {
    let batch = lock(connection, id).await?;
    if batch.state != BatchState::Proposed {
        return Err(ChangeError::Refused(format!(
            "batch {id} is {}; only a proposed batch is approved or rejected",
            state_words(batch.state)
        )));
    }
    Ok(batch)
}

/// Brings the approval item of `batch`'s project in line now that `batch` is decided.
async fn settle(
    connection: &mut AsyncPgConnection,
    batch: WorkBatch,
    by: Uuid,
    now: DateTime<Utc>,
) -> Result<Written, ChangeError> {
    let actor = approval_item::actor_for(by, batch.project_id);
    let approval_item =
        approval_item::refresh(connection, batch.project_id, Cause::Settled, actor, now).await?;
    Ok(Written {
        batch,
        approval_item,
    })
}

/// The fields `edits` changes on `proposed`, before and after, in the names the API gives them. A
/// value equal to the proposed one changes nothing. A priority and its scheduled time change
/// together, as a reprioritized entry records them.
fn changed_fields(proposed: &WorkBatch, edits: &Edits) -> (Map<String, Value>, Map<String, Value>) {
    let mut before = Map::new();
    let mut after = Map::new();
    if let Some(title) = edits
        .title
        .as_ref()
        .filter(|title| **title != proposed.title)
    {
        before.insert("title".to_owned(), json!(proposed.title));
        after.insert("title".to_owned(), json!(title));
    }
    if let Some(goal) = edits.goal.as_ref().filter(|goal| **goal != proposed.goal) {
        before.insert("goal".to_owned(), json!(proposed.goal));
        after.insert("goal".to_owned(), json!(goal));
    }
    if let Some(repositories) = &edits.repositories {
        let repositories = json!(repositories);
        if repositories != proposed.repositories {
            before.insert("repositories".to_owned(), proposed.repositories.clone());
            after.insert("repositories".to_owned(), repositories);
        }
    }
    if let Some((priority, scheduled_for)) = edits.priority
        && (priority, scheduled_for) != (proposed.priority, proposed.scheduled_for)
    {
        before.insert("priority".to_owned(), json!(proposed.priority));
        before.insert("scheduledFor".to_owned(), json!(proposed.scheduled_for));
        after.insert("priority".to_owned(), json!(priority));
        after.insert("scheduledFor".to_owned(), json!(scheduled_for));
    }
    (before, after)
}

#[cfg(test)]
mod tests;
