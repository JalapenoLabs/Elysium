// Copyright © 2026 Jalapeno Labs

//! Promises a person made in a conversation, and the follow-up each is owed once its work is done.
//! See `docs/engine.md`, Follow-ups.
//!
//! # A promise's life
//!
//! Segmenting finds a promise in one of the person's messages ([`record`]) and puts it on the
//! project whose work keeps it, `waiting`. Its Project Manager ties it to that work ([`tie`]): a
//! batch it submitted, or an action item, which stands for whatever is linked to it. When the work
//! finishes the promise comes `due`, in the transaction that finished it: a batch succeeding
//! (`work_batch::finish`) or an item resolving (`action_item::transition`, which is how a tracker
//! issue closing or a pull request merging reaches it through the link watcher). The Project
//! Manager then drafts the follow-up, an inbox item of the person's with a draft beside it
//! ([`mark_drafted`]), and the person decides:
//!
//! - **Send** posts the draft through `crate::drafts::send`, which keeps the promise ([`keep`])
//!   and resolves its item in the transaction that records the send.
//! - **Dismiss** dismisses the item, which marks the promise `dismissed`.
//! - Resolving the item by hand, having told them some other way, keeps it too.
//!
//! Every change that should wake a Project Manager writes its wake in the same transaction, so no
//! crash or lost signal loses one. The tests against Postgres are with the Project Managers' side
//! of it, in `crate::project_managers::promises`.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::schema::promises;
use crate::models::action_item::ActionItemState;
use crate::models::project_manager;
use crate::models::user::ELYSIA_USER_ID;

/// The longest recipient and summary kept, which `promises_recipient_length` and
/// `promises_summary_length` hold too.
pub const RECIPIENT_MAX_CHARACTERS: usize = 200;
pub const SUMMARY_MAX_CHARACTERS: usize = 500;

/// How many promises one page of a project's promises holds at most.
pub const MAX_PROMISE_PAGE: i64 = 200;

/// Where a promise stands.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::PromiseState"]
#[serde(rename_all = "snake_case")]
pub enum PromiseState {
    /// Recorded; its work has not finished, or it is tied to no work yet.
    Waiting,
    /// Its work finished; its Project Manager owes the follow-up.
    Due,
    /// The follow-up is drafted, for the person to send, edit, or dismiss.
    Drafted,
    /// The person sent the follow-up, or resolved its item.
    Kept,
    /// The person dismissed the follow-up.
    Dismissed,
}

/// A stored promise.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = promises, check_for_backend(diesel::pg::Pg))]
pub struct Promise {
    pub id: Uuid,
    pub project_id: Uuid,
    /// The person's message it was made in, which its follow-up answers.
    pub timeline_record_id: Uuid,
    /// Who it is for, by name.
    pub recipient: String,
    /// What was promised, as a short phrase.
    pub summary: String,
    pub state: PromiseState,
    pub work_batch_id: Option<Uuid>,
    pub work_item_id: Option<Uuid>,
    /// When its work finished.
    pub due_at: Option<DateTime<Utc>>,
    /// The item its follow-up is, once drafted.
    pub follow_up_item_id: Option<Uuid>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The work a promise waits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Work {
    /// A batch its Project Manager submitted, kept when it succeeds.
    Batch(Uuid),
    /// An action item, kept when it resolves.
    Item(Uuid),
}

impl Promise {
    /// The work it waits on, once its Project Manager tied it to some.
    pub const fn work(&self) -> Option<Work> {
        match (self.work_batch_id, self.work_item_id) {
            (Some(batch_id), _) => Some(Work::Batch(batch_id)),
            (None, Some(item_id)) => Some(Work::Item(item_id)),
            (None, None) => None,
        }
    }
}

/// A promise segmenting found.
#[derive(Debug, Clone, PartialEq, Insertable)]
#[diesel(table_name = promises)]
pub struct NewPromise {
    pub project_id: Uuid,
    pub timeline_record_id: Uuid,
    pub recipient: String,
    pub summary: String,
}

/// Records each of `found` as Elysia's, once per message, and wakes each new one's Project
/// Manager. A message already holding a promise keeps the one it has, so a conversation
/// read again as it grows records nothing twice. Answers the promises newly recorded. Call it in
/// the transaction that attributes the messages.
///
/// # Errors
/// Propagates any database error, including a recipient or summary out of bounds.
pub async fn record(
    connection: &mut AsyncPgConnection,
    found: &[NewPromise],
) -> QueryResult<Vec<Promise>> {
    let mut recorded = Vec::new();
    for promise in found {
        let inserted = diesel::insert_into(promises::table)
            .values((
                promises::id.eq(Uuid::now_v7()),
                promise,
                promises::created_by.eq(ELYSIA_USER_ID),
            ))
            .on_conflict(promises::timeline_record_id)
            .do_nothing()
            .returning(Promise::as_returning())
            .get_result(connection)
            .await
            .optional()?;
        let Some(inserted) = inserted else {
            continue;
        };
        project_manager::wake_for_promise(
            connection,
            inserted.project_id,
            inserted.id,
            ELYSIA_USER_ID,
        )
        .await?;
        recorded.push(inserted);
    }
    Ok(recorded)
}

/// The promises among `ids`, in no particular order.
///
/// # Errors
/// Propagates any database error.
pub async fn find_many(
    connection: &mut AsyncPgConnection,
    ids: &[Uuid],
) -> QueryResult<Vec<Promise>> {
    promises::table
        .filter(promises::id.eq_any(ids))
        .select(Promise::as_select())
        .load(connection)
        .await
}

/// The promise whose follow-up is item `item_id`, if any.
///
/// # Errors
/// Propagates any database error.
pub async fn find_by_follow_up(
    connection: &mut AsyncPgConnection,
    item_id: Uuid,
) -> QueryResult<Option<Promise>> {
    promises::table
        .filter(promises::follow_up_item_id.eq(item_id))
        .select(Promise::as_select())
        .first(connection)
        .await
        .optional()
}

/// `project_id`'s latest promises in `states`, or every state when it is empty, newest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list_for_project(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    states: &[PromiseState],
    limit: i64,
) -> QueryResult<Vec<Promise>> {
    let mut query = promises::table
        .filter(promises::project_id.eq(project_id))
        .select(Promise::as_select())
        .order(promises::id.desc())
        .limit(limit.clamp(1, MAX_PROMISE_PAGE))
        .into_boxed();
    if !states.is_empty() {
        query = query.filter(promises::state.eq_any(states.to_vec()));
    }
    query.load(connection).await
}

/// The promises still waiting on any of the batches `batch_ids`.
///
/// # Errors
/// Propagates any database error.
pub async fn waiting_on_batches(
    connection: &mut AsyncPgConnection,
    batch_ids: &[Uuid],
) -> QueryResult<Vec<Promise>> {
    promises::table
        .filter(promises::work_batch_id.eq_any(batch_ids))
        .filter(promises::state.eq(PromiseState::Waiting))
        .select(Promise::as_select())
        .order(promises::id.asc())
        .load(connection)
        .await
}

/// Ties `project_id`'s waiting promise `id` to `work`, replacing what it waited on. When the work
/// already finished (`finished`), it comes due at once, waking its Project Manager as `by`.
/// `None` when the project has no such waiting promise.
///
/// # Errors
/// Propagates any database error.
pub async fn tie(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    id: Uuid,
    work: Work,
    finished: bool,
    by: Uuid,
    now: DateTime<Utc>,
) -> QueryResult<Option<Promise>> {
    let (batch_id, item_id) = match work {
        Work::Batch(batch_id) => (Some(batch_id), None),
        Work::Item(item_id) => (None, Some(item_id)),
    };
    let (state, due_at) = if finished {
        (PromiseState::Due, Some(now))
    } else {
        (PromiseState::Waiting, None)
    };
    let tied = diesel::update(
        promises::table
            .find(id)
            .filter(promises::project_id.eq(project_id))
            .filter(promises::state.eq(PromiseState::Waiting)),
    )
    .set((
        promises::work_batch_id.eq(batch_id),
        promises::work_item_id.eq(item_id),
        promises::state.eq(state),
        promises::due_at.eq(due_at),
    ))
    .returning(Promise::as_returning())
    .get_result(connection)
    .await
    .optional()?;
    if let Some(promise) = &tied
        && finished
    {
        project_manager::wake_for_promise(connection, project_id, promise.id, by).await?;
    }
    Ok(tied)
}

/// Every promise waiting on batch `batch_id` comes due, waking its Project Manager as `by`. Call
/// it in the transaction that records the batch succeeding.
///
/// # Errors
/// Propagates any database error.
pub async fn come_due_for_batch(
    connection: &mut AsyncPgConnection,
    batch_id: Uuid,
    by: Uuid,
    now: DateTime<Utc>,
) -> QueryResult<Vec<Promise>> {
    let due: Vec<Promise> = diesel::update(
        promises::table
            .filter(promises::work_batch_id.eq(batch_id))
            .filter(promises::state.eq(PromiseState::Waiting)),
    )
    .set((
        promises::state.eq(PromiseState::Due),
        promises::due_at.eq(now),
    ))
    .returning(Promise::as_returning())
    .get_results(connection)
    .await?;
    wake_each(connection, &due, by).await?;
    Ok(due)
}

/// What item `item_id` moving to `state` means for promises, as `by`, at `now`: the promises
/// waiting on it come due when it resolves, and the promise whose follow-up it is is kept when it
/// resolves and dismissed when it is dismissed. Accepting or reopening an item changes no
/// promise: a dismissed promise whose item is reopened is kept again only once its follow-up is
/// sent or its item resolved. Call it in the transaction that moves the item.
///
/// # Errors
/// Propagates any database error.
pub async fn follow_item(
    connection: &mut AsyncPgConnection,
    item_id: Uuid,
    state: ActionItemState,
    by: Uuid,
    now: DateTime<Utc>,
) -> QueryResult<()> {
    match state {
        ActionItemState::Resolved => {
            let due: Vec<Promise> = diesel::update(
                promises::table
                    .filter(promises::work_item_id.eq(item_id))
                    .filter(promises::state.eq(PromiseState::Waiting)),
            )
            .set((
                promises::state.eq(PromiseState::Due),
                promises::due_at.eq(now),
            ))
            .returning(Promise::as_returning())
            .get_results(connection)
            .await?;
            wake_each(connection, &due, by).await?;
            move_follow_up(
                connection,
                item_id,
                &[PromiseState::Drafted, PromiseState::Dismissed],
                PromiseState::Kept,
            )
            .await
        }
        ActionItemState::Dismissed => {
            move_follow_up(
                connection,
                item_id,
                &[PromiseState::Drafted],
                PromiseState::Dismissed,
            )
            .await
        }
        ActionItemState::Inbox | ActionItemState::Open => Ok(()),
    }
}

/// Moves the promise whose follow-up is item `item_id` from any of `from` to `to`.
async fn move_follow_up(
    connection: &mut AsyncPgConnection,
    item_id: Uuid,
    from: &[PromiseState],
    to: PromiseState,
) -> QueryResult<()> {
    diesel::update(
        promises::table
            .filter(promises::follow_up_item_id.eq(item_id))
            .filter(promises::state.eq_any(from.to_vec())),
    )
    .set(promises::state.eq(to))
    .execute(connection)
    .await
    .map(drop)
}

/// Marks due promise `id` drafted, its follow-up item `follow_up_item_id`. `None` when it is not
/// due. Call it in the transaction that writes the draft and the item.
///
/// # Errors
/// Propagates any database error.
pub async fn mark_drafted(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    follow_up_item_id: Uuid,
) -> QueryResult<Option<Promise>> {
    diesel::update(
        promises::table
            .find(id)
            .filter(promises::state.eq(PromiseState::Due)),
    )
    .set((
        promises::state.eq(PromiseState::Drafted),
        promises::follow_up_item_id.eq(follow_up_item_id),
    ))
    .returning(Promise::as_returning())
    .get_result(connection)
    .await
    .optional()
}

/// Marks promise `id` kept: its follow-up was sent. Call it in the transaction that records the
/// send.
///
/// # Errors
/// Propagates any database error, including a promise that does not exist.
pub async fn keep(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Promise> {
    diesel::update(promises::table.find(id))
        .set(promises::state.eq(PromiseState::Kept))
        .returning(Promise::as_returning())
        .get_result(connection)
        .await
}

/// The projects whose promises wait on, or are followed up by, any of the items `item_ids`:
/// the Project Managers a write to those items changes the page of.
///
/// # Errors
/// Propagates any database error.
pub async fn projects_following(
    connection: &mut AsyncPgConnection,
    item_ids: &[Uuid],
) -> QueryResult<Vec<Uuid>> {
    promises::table
        .filter(
            promises::work_item_id
                .eq_any(item_ids)
                .or(promises::follow_up_item_id.eq_any(item_ids)),
        )
        .select(promises::project_id)
        .distinct()
        .load(connection)
        .await
}

async fn wake_each(
    connection: &mut AsyncPgConnection,
    promises: &[Promise],
    by: Uuid,
) -> QueryResult<()> {
    for promise in promises {
        project_manager::wake_for_promise(connection, promise.project_id, promise.id, by).await?;
    }
    Ok(())
}
