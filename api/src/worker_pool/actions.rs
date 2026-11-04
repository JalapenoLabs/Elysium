// Copyright © 2026 Jalapeno Labs

//! What people, Elysia, and Project Managers do to a batch while it waits: run it next, change
//! its priority, pause or resume it, and cancel it. The work queue's routes, Elysia's tools, and
//! a Project Manager's tools all call these, so a change made anywhere is checked, recorded in
//! the batch's history (`crate::models::work_batch`), announced, and signalled alike.

use chrono::Utc;
use diesel_async::AsyncPgConnection;
use uuid::Uuid;

use crate::models::work_batch::{self, ChangeError, Changes, RESULT_MAX_CHARACTERS, WorkBatch};
use crate::realtime::{EventBus, ServerEvent, Signal};

/// Applies `changes` to batch `id` as `by`, then announces the batch and tells the pool, which
/// may hand it a slot now or fill the one it gave back. A change that moved nothing announces
/// nothing.
///
/// # Errors
/// As [`work_batch::change`], and [`ChangeError::Invalid`] for no change at all.
pub async fn change(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    id: Uuid,
    changes: Changes,
    by: Uuid,
) -> Result<WorkBatch, ChangeError> {
    if changes.is_empty() {
        return Err(ChangeError::Invalid(
            "name a change: its priority, whether it is paused, or whether it runs next".to_owned(),
        ));
    }
    let (batch, moved) = work_batch::change(connection, id, changes, by, Utc::now()).await?;
    if moved {
        announce(events, &batch);
        events.signal(&Signal::WorkerPool);
    }
    Ok(batch)
}

/// Withdraws batch `id` as `by`, saying `reason`, while no worker has started on it; with
/// `project_id`, only a batch of that project. Announces it, wakes its Project Manager unless it
/// withdrew the batch itself, and tells the pool, which fills a slot it gave back.
///
/// # Errors
/// [`ChangeError::Invalid`] for a reason with no text or over 20,000 characters, and as
/// [`work_batch::cancel`].
pub async fn cancel(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    id: Uuid,
    project_id: Option<Uuid>,
    reason: &str,
    by: Uuid,
) -> Result<WorkBatch, ChangeError> {
    let reason = reason.trim();
    if reason.is_empty() || reason.chars().count() > RESULT_MAX_CHARACTERS {
        return Err(ChangeError::Invalid(format!(
            "reason must hold text, at most {RESULT_MAX_CHARACTERS} characters"
        )));
    }
    let cancelled = work_batch::cancel(connection, id, project_id, reason, by).await?;
    announce(events, &cancelled);
    events.signal(&Signal::WorkerPool);
    events.signal(&Signal::ProjectManagers);
    Ok(cancelled)
}

/// Tells every client that `batch` moved: the work queue and its own page by
/// `workBatch.changed`, and its project's page, whose Work batches tab lists it, by
/// `projectManager.changed`.
pub fn announce(events: &EventBus, batch: &WorkBatch) {
    events.publish(&ServerEvent::WorkBatchChanged {
        id: batch.id,
        project_id: batch.project_id,
        state: batch.state,
    });
    events.publish(&ServerEvent::ProjectManagerChanged {
        project_id: batch.project_id,
    });
}
