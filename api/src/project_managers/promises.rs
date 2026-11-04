// Copyright © 2026 Jalapeno Labs

//! A Project Manager's part in keeping its project's promises: tying each to the work that keeps
//! it, and drafting its follow-up once that work is done. See `docs/engine.md`, Follow-ups, and
//! `crate::models::promise` for a promise's life.
//!
//! Nothing a Project Manager names is taken on trust. A promise is tied only to a batch of its own
//! project's that may still succeed, or to a live action item in its project that may still be
//! resolved. A follow-up is drafted only for a promise whose work is done, as the person who made
//! it, replying to the message they made it in through what answers that message's kind
//! (`crate::drafts::reply_draft`), and it becomes an inbox item of theirs in the same transaction.
//! Nothing is sent: the person sends it, edits it, or dismisses it.

use chrono::Utc;
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::action_items::{Actor, WorkError};
use crate::database::schema::action_item_projects;
use crate::drafts::{self, Reply};
use crate::errors::ApiError;
use crate::models::action_item::{
    self, ActionItem, ActionItemPriority, ActionItemState, NewActionItem, Owner,
};
use crate::models::draft;
use crate::models::promise::{self, Promise, PromiseState, Work};
use crate::models::timeline_record::{self, TimelineRecord};
use crate::models::user;
use crate::models::work_batch::{self, BatchState};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::routes::v1::action_items::publish_item_write;

/// The longest follow-up item title, matching what a changeset's item may carry.
const TITLE_MAX_CHARACTERS: usize = 500;

/// How much of the message a promise was made in its follow-up item's notes quote.
const QUOTE_MAX_CHARACTERS: usize = 1_000;

/// Why a Project Manager's request about a promise was refused, in words it can act on.
#[derive(Debug, thiserror::Error)]
pub enum PromiseError {
    #[error("{0}")]
    Refused(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl From<diesel::result::Error> for PromiseError {
    fn from(error: diesel::result::Error) -> Self {
        Self::Internal(error.into())
    }
}

impl From<WorkError> for PromiseError {
    fn from(error: WorkError) -> Self {
        match error {
            WorkError::Database(error) => error.into(),
            WorkError::Conflict(reason) | WorkError::Invalid(reason) => {
                Self::Refused(reason.to_owned())
            }
            WorkError::Refused(reason) => Self::Refused(reason),
        }
    }
}

/// Ties `project_id`'s waiting promise `promise_id` to `work`, and announces it. Work that already
/// finished makes it due at once, so its follow-up is drafted on the Project Manager's next turn.
///
/// # Errors
/// [`PromiseError::Refused`] for a promise that is not waiting on this project, a batch of another
/// project or one that failed or was cancelled, or an item outside the project, deleted, or
/// dismissed; any database error.
pub async fn tie(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    project_id: Uuid,
    promise_id: Uuid,
    work: Work,
) -> Result<Promise, PromiseError> {
    let finished = match work {
        Work::Batch(batch_id) => batch_finished(connection, project_id, batch_id).await?,
        Work::Item(item_id) => item_finished(connection, project_id, item_id).await?,
    };
    let tied = promise::tie(
        connection,
        project_id,
        promise_id,
        work,
        finished,
        user::PROJECT_MANAGER_USER_ID,
        Utc::now(),
    )
    .await?
    .ok_or_else(|| {
        PromiseError::Refused(format!(
            "promise {promise_id} is not one of this project's waiting promises; call \
             manager_promises for where each stands"
        ))
    })?;
    events.publish(&ServerEvent::ProjectManagerChanged { project_id });
    if finished {
        events.signal(&Signal::ProjectManagers);
    }
    Ok(tied)
}

/// Whether batch `batch_id` of `project_id` already succeeded. Refuses one of another project, and
/// one that failed or was cancelled, which will never keep a promise.
async fn batch_finished(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    batch_id: Uuid,
) -> Result<bool, PromiseError> {
    let batch = work_batch::find_many(connection, &[batch_id])
        .await?
        .into_iter()
        .find(|batch| batch.project_id == project_id)
        .ok_or_else(|| {
            PromiseError::Refused(format!(
                "batch {batch_id} is not one of this project's; call manager_batches for the ones \
                 that are"
            ))
        })?;
    match batch.state {
        BatchState::Queued | BatchState::Paused | BatchState::Assigned | BatchState::Running => {
            Ok(false)
        }
        BatchState::Succeeded => Ok(true),
        BatchState::Failed | BatchState::Cancelled => Err(PromiseError::Refused(format!(
            "batch {batch_id} did not succeed and never will; tie the promise to the work that \
             replaces it"
        ))),
    }
}

/// Whether item `item_id`, in `project_id`, is already resolved. Refuses one outside the project,
/// deleted, or dismissed, which will never keep a promise.
async fn item_finished(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    item_id: Uuid,
) -> Result<bool, PromiseError> {
    let in_project: i64 = action_item_projects::table
        .filter(action_item_projects::project_id.eq(project_id))
        .filter(action_item_projects::action_item_id.eq(item_id))
        .count()
        .get_result(connection)
        .await?;
    let item = if in_project == 0 {
        None
    } else {
        Some(action_item::find(connection, item_id).await?)
    };
    let Some(item) = item.filter(|item| item.deleted_at.is_none()) else {
        return Err(PromiseError::Refused(format!(
            "action item {item_id} is not in this project or was deleted; call work_items for \
             the ones that are"
        )));
    };
    match item.state {
        ActionItemState::Inbox | ActionItemState::Open => Ok(false),
        ActionItemState::Resolved => Ok(true),
        ActionItemState::Dismissed => Err(PromiseError::Refused(format!(
            "action item {item_id} was dismissed and will not be done; tie the promise to the \
             work that keeps it"
        ))),
    }
}

/// What drafting a follow-up wrote, once announced: the promise, drafted, and its item.
#[derive(Debug)]
pub struct FollowUp {
    pub promise: Promise,
    pub item: ActionItem,
}

/// Drafts the follow-up `project_id`'s due promise `promise_id` is owed, saying `message` as the
/// person who made it: a draft of theirs answering the message they made it in, and an inbox item
/// of theirs that sends, edits, or dismisses it, written together, then announced. Nothing is sent.
///
/// # Errors
/// [`PromiseError::Refused`] for a promise that is not due on this project, a message that can no
/// longer be answered, or a `message` out of bounds; any database error.
pub async fn draft_follow_up(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    project_id: Uuid,
    promise_id: Uuid,
    message: &str,
) -> Result<FollowUp, PromiseError> {
    let due = promise::find_many(connection, &[promise_id])
        .await?
        .into_iter()
        .find(|candidate| {
            candidate.project_id == project_id && candidate.state == PromiseState::Due
        })
        .ok_or_else(|| {
            PromiseError::Refused(format!(
                "promise {promise_id} is not one of this project's promises whose work is done; \
                 call manager_promises for where each stands"
            ))
        })?;
    let record = timeline_record::find_many(connection, &[due.timeline_record_id])
        .await?
        .pop()
        .ok_or_else(|| {
            PromiseError::Refused("the message this promise was made in is gone".to_owned())
        })?;
    let person = user::find(connection, record.created_by).await?;
    let reply = Reply {
        record_id: record.id,
        // The kind's own first place, as the person's reply box starts with.
        place: None,
        mail_account_id: None,
        body: message.trim().to_owned(),
    };
    let mut new_draft = drafts::reply_draft(connection, &person, reply)
        .await
        .map_err(refusal)?;
    new_draft.promise_id = Some(due.id);
    let new_item = NewActionItem {
        title: item_title(&due),
        notes: item_notes(&due, &record),
        state: ActionItemState::Inbox,
        priority: ActionItemPriority::Normal,
        due_at: None,
        owner: Owner::User,
        project_ids: vec![project_id],
        initiative_ids: Vec::new(),
    };

    let now = Utc::now();
    let (followed_up, created, raised) = connection
        .transaction(async |connection| {
            let raised =
                action_item::create(connection, new_item, Actor::ProjectManager(project_id), now)
                    .await?;
            let created = draft::create(connection, new_draft).await?;
            let followed_up = promise::mark_drafted(connection, due.id, raised.record.id)
                .await?
                .ok_or_else(|| {
                    PromiseError::Refused(
                        "this promise changed meanwhile; call manager_promises".to_owned(),
                    )
                })?;
            Ok::<_, PromiseError>((followed_up, created, raised))
        })
        .await?;

    events.publish(&ServerEvent::DraftChanged {
        id: created.id,
        created_by: created.created_by,
    });
    let item = raised.record.clone();
    publish_item_write(events, connection, raised, &[], now)
        .await
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    events.publish(&ServerEvent::ProjectManagerChanged { project_id });
    Ok(FollowUp {
        promise: followed_up,
        item,
    })
}

/// The follow-up item's title: who to tell, and what.
fn item_title(promise: &Promise) -> String {
    format!("Follow up with {}: {}", promise.recipient, promise.summary)
        .chars()
        .take(TITLE_MAX_CHARACTERS)
        .collect()
}

/// The follow-up item's notes: where the promise was made, and what the person said.
fn item_notes(promise: &Promise, record: &TimelineRecord) -> String {
    let said: String = record
        .body
        .trim()
        .chars()
        .take(QUOTE_MAX_CHARACTERS)
        .collect();
    format!(
        "You promised {} in {}:\n\n> {}",
        promise.recipient,
        record.title,
        said.replace('\n', "\n> ")
    )
}

/// A draft that could not be written, as the reason the Project Manager reads.
fn refusal(error: ApiError) -> PromiseError {
    match error {
        ApiError::BadRequest(message) | ApiError::CodedBadRequest { message, .. } => {
            PromiseError::Refused(message)
        }
        ApiError::NotFound => PromiseError::Refused(
            "the message this promise was made in can no longer be answered: its connector was \
             removed or is no longer its person's"
                .to_owned(),
        ),
        other => PromiseError::Internal(anyhow::anyhow!("{other}")),
    }
}

#[cfg(test)]
mod tests;
