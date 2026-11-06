// Copyright © 2026 Jalapeno Labs

//! What people, Elysia, and Project Managers do to a batch while it waits: approve or reject a
//! proposed one, run a queued one next, change its priority, pause or resume it, and cancel it.
//! The work queue's routes, Elysia's tools, and a Project Manager's tools all call these, so a
//! change made anywhere is checked, recorded in the batch's history (`crate::models::work_batch`),
//! announced, and signalled alike.
//!
//! Approving and rejecting take a [`MemberUser`]: a person signed in, which no machine user (the
//! system user, Elysia, the coding agent, the Project Managers) can be. Agents' tools hold no
//! session, so none of them can decide on a proposal, however it is called.

use chrono::Utc;
use diesel_async::AsyncPgConnection;
use tracing::{Level, event};
use uuid::Uuid;

use crate::auth::MemberUser;
use crate::models::work_batch::approval::{self, Edits};
use crate::models::work_batch::{
    self, ChangeError, Changes, RESULT_MAX_CHARACTERS, WorkBatch, Written,
};
use crate::project_managers::batches;
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::routes::v1::action_items::publish_item_write;

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

/// Approves proposed batch `id` as `person`, first applying `edits`, which are held to the rules
/// every batch is. Announces it and its project's approval item, and tells the pool, which may
/// hand it a slot at once.
///
/// # Errors
/// [`ChangeError::Invalid`] for an edit the rules refuse, naming it, and as
/// [`approval::approve`].
pub async fn approve(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    id: Uuid,
    edits: Edits,
    person: &MemberUser,
) -> Result<WorkBatch, ChangeError> {
    let now = Utc::now();
    let edits = batches::check_edits(edits, now).map_err(ChangeError::Invalid)?;
    let written = approval::approve(connection, id, edits, person.id(), now).await?;
    let approved = publish(connection, events, written).await;
    events.signal(&Signal::WorkerPool);
    Ok(approved)
}

/// Rejects proposed batch `id` as `person`, saying `reason`, which its Project Manager is told on
/// its next turn. Announces it and its project's approval item, and tells the Project Managers'
/// dispatcher.
///
/// # Errors
/// [`ChangeError::Invalid`] for a reason with no text or over 20,000 characters, and as
/// [`approval::reject`].
pub async fn reject(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    id: Uuid,
    reason: &str,
    person: &MemberUser,
) -> Result<WorkBatch, ChangeError> {
    let reason = checked_reason(reason)?;
    let written = approval::reject(connection, id, reason, person.id(), Utc::now()).await?;
    let rejected = publish(connection, events, written).await;
    events.signal(&Signal::ProjectManagers);
    Ok(rejected)
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
    let reason = checked_reason(reason)?;
    let written = work_batch::cancel(connection, id, project_id, reason, by).await?;
    let cancelled = publish(connection, events, written).await;
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

/// Announces what a committed write left: the batch, and its project's approval item when the
/// write changed it. Answers the batch.
pub async fn publish(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    written: Written,
) -> WorkBatch {
    announce(events, &written.batch);
    let Some(item) = written.approval_item else {
        return written.batch;
    };
    let item_id = item.record.id;
    // The item is written either way, and a client that missed it reads it on its next load, so a
    // failure is logged rather than failing a change already made.
    if let Err(error) = publish_item_write(events, connection, item, &[], Utc::now()).await {
        event!(
            name: "worker_pool.approval_item.unpublished",
            Level::WARN,
            work_batch.id = %written.batch.id,
            action_item.id = %item_id,
            error.message = %error,
            "a project's approval item was written but could not be announced",
        );
    }
    written.batch
}

/// `reason`, trimmed, when it holds text and fits a batch's result.
fn checked_reason(reason: &str) -> Result<&str, ChangeError> {
    let reason = reason.trim();
    if reason.is_empty() || reason.chars().count() > RESULT_MAX_CHARACTERS {
        return Err(ChangeError::Invalid(format!(
            "reason must hold text, at most {RESULT_MAX_CHARACTERS} characters"
        )));
    }
    Ok(reason)
}

#[cfg(test)]
mod tests {
    use crate::auth::MemberUser;
    use crate::models::user::{
        self, CODING_AGENT_USER_ID, ELYSIA_USER_ID, PROJECT_MANAGER_USER_ID, SYSTEM_USER_ID,
    };
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    /// Approving and rejecting take a `MemberUser`, so no seeded machine can decide on a proposal
    /// whatever calls them: each is refused as one, and a person is not.
    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn only_a_person_can_be_the_one_who_decides() {
        let (_url, mut connection) = migrated_database().await;
        for machine in [
            SYSTEM_USER_ID,
            ELYSIA_USER_ID,
            CODING_AGENT_USER_ID,
            PROJECT_MANAGER_USER_ID,
        ] {
            let found = user::find(&mut connection, machine).await.expect("seeded");
            assert!(found.is_machine(), "{machine}");
            assert!(
                MemberUser::check(found).is_err(),
                "{machine} must never approve a batch"
            );
        }
        let person = user::find(&mut connection, TEST_PERSON_ID)
            .await
            .expect("the test person");
        MemberUser::check(person).expect("a person decides");
    }
}
