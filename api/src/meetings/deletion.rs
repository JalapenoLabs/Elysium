// Copyright © 2026 Jalapeno Labs

//! Deleting a meeting, and restoring it: one, many, or with the connector that read them. See
//! `docs/meetings.md`, Deleting.
//!
//! # Soft, and whole
//!
//! Nothing is deleted for good yet, so a deletion only marks what it takes, and restoring brings
//! back all of it:
//!
//! - **The meeting** keeps when it was deleted (`meetings.deleted_at`). It is listed nowhere but
//!   among its person's deleted meetings, takes no change (every route answers `409
//!   meeting_deleted`), and is processed no further. Its captures stay, so its connector's next
//!   listing finds it known and never stores it again; its recording and clips stay in storage,
//!   as a deleted page keeps its files.
//! - **Its segments** go with it, stamped with the meeting's own `deleted_at`
//!   (`timeline_record::delete_of_meeting`): off every project's timeline, out of every Project
//!   Manager's reading, and out of the knowledge index, each keeping its attributions and their
//!   history. Restoring brings back exactly the records that stamp names, and indexes each again.
//! - **Its run stops.** Every meeting step fails for good on a deleted meeting with
//!   [`STOPPED`], at its start and again under the meeting's lock where it would raise an inbox
//!   item or publish, so a deleted meeting asks nothing of its person and puts nothing on a
//!   timeline. The inbox item it raised already, asking to approve its corrected transcript, is
//!   dismissed as the person deleting it (`correct::dismiss_review`). Restoring retries the step
//!   that stopped: the failed step of the meeting's newest run of each meeting workflow that failed
//!   after the deletion, whatever it failed with, since nobody saw a meeting that was listed
//!   nowhere. A run that failed before the deletion waits for its person as it did, and a held run
//!   stays held, its meeting's row and page saying it waits.
//!
//! Each deletion and restore is in the meeting's history (`deleted`, `restored`) with who did it.
//! Neither is undone through the history: restoring is the undo of deleting, and deleting again
//! the undo of restoring.
//!
//! # Who, and which meetings
//!
//! A meeting is found as every meeting route finds it (`meeting::find_owned`), then locked, so a
//! deletion waits for an edit, a merge, a step's publishing, or an upload's finish, and they for
//! it. A meeting merged into another is refused (`meeting_merged`): its captures live on in the
//! meeting kept, which is the one to delete. A meeting whose connector was removed is deleted like
//! any other.
//!
//! Many meetings change in one transaction, each in a savepoint, in the order of their ids, as a
//! bulk change to action items does (`models::action_item_bulk`): one refused is reported with
//! its reason while the rest apply, and a fault inside Elysium writes nothing. One change names at
//! most [`MAX_MEETINGS`]; a caller with more sends them in parts.

use chrono::{DateTime, Utc};
use diesel::result::Error as DieselError;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{Level, event};
use uuid::Uuid;

use super::{correct, workflow};
use crate::models::action_item::ActionItem;
use crate::models::action_item_event::Recorded;
use crate::models::agent::Author;
use crate::models::meeting::{self, Meeting};
use crate::models::meeting_event::{self, Change, EventKind, MeetingEvent};
use crate::models::step_job::{self, StepActionError, StepJobState};
use crate::models::timeline_record::{self, TimelineRecord};
use crate::models::workflow_run::{self, Actor, Settled, WorkflowRunState};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::routes::v1::action_items::publish_item_write;
use crate::workflows;

/// The reason every meeting step fails with for a deleted meeting, which is what stops its run.
pub const STOPPED: &str =
    "This meeting was deleted, so its run stopped here. Restoring it carries the run on.";

/// The most meetings one change names.
///
/// Every meeting is written in one transaction, so the limit keeps that transaction, and the burst
/// of events it publishes afterwards, short. It is far above a page of the Meetings table, and a
/// selection across pages of a larger list asks in parts.
pub const MAX_MEETINGS: usize = 1000;

/// What to do to every meeting named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
#[expect(
    clippy::empty_enum_variants_with_brackets,
    reason = "serde refuses unknown fields only on a struct variant, as on every other body"
)]
pub enum BulkChange {
    Delete {},
    Restore {},
}

/// Why a meeting refused the change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalKind {
    /// No meeting of the person's has this id.
    NotFound,
    /// It was merged into another, which holds its captures and is the one to change.
    Merged,
    /// Deleting a meeting deleted already.
    Deleted,
    /// Restoring a meeting that is not deleted.
    NotDeleted,
}

impl RefusalKind {
    /// The code a client reads, as the single-meeting routes answer it.
    pub const fn code(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::Merged => "meeting_merged",
            Self::Deleted => "meeting_deleted",
            Self::NotDeleted => "meeting_not_deleted",
        }
    }

    /// Why, in words for the person.
    pub const fn message(self) -> &'static str {
        match self {
            Self::NotFound => "no meeting of yours has this id",
            Self::Merged => "this meeting was merged into another, so it changes there",
            Self::Deleted => "this meeting is deleted already",
            Self::NotDeleted => "this meeting is not deleted",
        }
    }
}

/// A meeting the change did not apply to, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Refusal {
    pub meeting_id: Uuid,
    pub code: &'static str,
    pub message: &'static str,
}

/// What the change did to one meeting.
#[derive(Debug)]
pub struct Changed {
    /// The meeting as it now stands.
    pub meeting: Meeting,
    /// Its `deleted` or `restored` entry.
    pub entry: MeetingEvent,
    /// The segments that went with it, or came back.
    pub records: Vec<TimelineRecord>,
    /// The runs restoring it carried on.
    pub runs: Vec<Settled>,
    /// The inbox item deleting it dismissed, which asked to approve its corrected transcript.
    pub dismissed: Option<Recorded<ActionItem>>,
}

/// What a change did.
#[derive(Debug, Default)]
pub struct Outcome {
    pub changed: Vec<Changed>,
    pub refused: Vec<Refusal>,
}

/// How one meeting's change ended inside its savepoint.
enum Failure {
    Refused(RefusalKind),
    Fault(DieselError),
}

impl From<DieselError> for Failure {
    fn from(error: DieselError) -> Self {
        Self::Fault(error)
    }
}

/// Applies `change` to every meeting in `meeting_ids`, at most [`MAX_MEETINGS`], as `person` at
/// `at`. Each is changed once, in the order of their ids, so two changes over the same meetings lock
/// them in one order. `removed_with` names the connector the meetings are deleted with, which their
/// history records. Nothing is announced: the caller does, once it commits ([`announce`]).
///
/// # Errors
/// Propagates any database error, which writes nothing. A meeting refused is reported in
/// [`Outcome::refused`].
pub async fn change_many(
    connection: &mut AsyncPgConnection,
    meeting_ids: &[Uuid],
    change: BulkChange,
    person: Uuid,
    at: DateTime<Utc>,
    removed_with: Option<Uuid>,
) -> Result<Outcome, DieselError> {
    let author = Author {
        user_id: person,
        agent: None,
        at,
    };
    let mut ordered = meeting_ids.to_vec();
    ordered.sort_unstable();
    ordered.dedup();
    connection
        .transaction(async move |connection| {
            let mut outcome = Outcome::default();
            for meeting_id in ordered {
                let attempt = connection
                    .transaction(async move |connection| match change {
                        BulkChange::Delete {} => {
                            delete_one(connection, meeting_id, author, removed_with).await
                        }
                        BulkChange::Restore {} => restore_one(connection, meeting_id, author).await,
                    })
                    .await;
                match attempt {
                    Ok(changed) => outcome.changed.push(changed),
                    Err(Failure::Refused(kind)) => outcome.refused.push(Refusal {
                        meeting_id,
                        code: kind.code(),
                        message: kind.message(),
                    }),
                    Err(Failure::Fault(error)) => return Err(error),
                }
            }
            Ok(outcome)
        })
        .await
}

/// Meeting `meeting_id`, the person's, locked for the change, or why it refuses: not found, or
/// merged into another.
async fn lock_changeable(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    person: Uuid,
) -> Result<Meeting, Failure> {
    // Found as every meeting route finds it, so who may change it is decided in one place.
    match meeting::find_owned(connection, meeting_id, person).await {
        Ok(_) => {}
        Err(DieselError::NotFound) => return Err(Failure::Refused(RefusalKind::NotFound)),
        Err(error) => return Err(error.into()),
    }
    // The lock every change to a meeting takes, read again under it.
    let locked = meeting::lock(connection, meeting_id).await?;
    if locked.merged_into.is_some() {
        return Err(Failure::Refused(RefusalKind::Merged));
    }
    Ok(locked)
}

/// Deletes meeting `meeting_id` with its segments, as `author`, recording it.
async fn delete_one(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    author: Author<'_>,
    removed_with: Option<Uuid>,
) -> Result<Changed, Failure> {
    let locked = lock_changeable(connection, meeting_id, author.user_id).await?;
    if locked.deleted_at.is_some() {
        return Err(Failure::Refused(RefusalKind::Deleted));
    }
    let deleted = meeting::set_deleted(connection, meeting_id, Some(author.at)).await?;
    // The stamp as Postgres stored it, to the microsecond, is what restoring matches the records
    // by, so the records take it from the meeting rather than from the clock.
    let stamp = deleted
        .deleted_at
        .expect("a meeting just deleted has its deleted_at");
    let records = timeline_record::delete_of_meeting(connection, meeting_id, stamp).await?;
    // Nothing asks its person to review what they deleted; a run held for it stays held.
    let dismissed =
        correct::dismiss_review(connection, meeting_id, author.user_id, author.at).await?;
    let entry = meeting_event::record(
        connection,
        Change {
            id: Uuid::now_v7(),
            meeting_id,
            kind: EventKind::Deleted,
            author,
            data: json!({
                "segments": records.len(),
                "connectorId": removed_with,
            }),
            undid: None,
        },
    )
    .await?;
    Ok(Changed {
        meeting: deleted,
        entry,
        records,
        runs: Vec::new(),
        dismissed,
    })
}

/// Restores meeting `meeting_id` with the segments its deletion took, as `author`, carries on the
/// run the deletion stopped, and records it.
async fn restore_one(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    author: Author<'_>,
) -> Result<Changed, Failure> {
    let locked = lock_changeable(connection, meeting_id, author.user_id).await?;
    let Some(deleted_at) = locked.deleted_at else {
        return Err(Failure::Refused(RefusalKind::NotDeleted));
    };
    let restored = meeting::set_deleted(connection, meeting_id, None).await?;
    let records = timeline_record::restore_of_meeting(connection, meeting_id, deleted_at).await?;
    let runs = carry_on(connection, meeting_id, deleted_at, author).await?;
    let entry = meeting_event::record(
        connection,
        Change {
            id: Uuid::now_v7(),
            meeting_id,
            kind: EventKind::Restored,
            author,
            data: json!({
                "deletedAt": deleted_at,
                "segments": records.len(),
                "runRetried": !runs.is_empty(),
            }),
            undid: None,
        },
    )
    .await?;
    Ok(Changed {
        meeting: restored,
        entry,
        records,
        runs,
        dismissed: None,
    })
}

/// Retries, as `author`, the step that stopped each of meeting `meeting_id`'s runs while it was
/// deleted from `deleted_at`: the failed step of the newest run of the meeting workflow, and of
/// the alignment workflow a merge or a split starts, that failed since then. Answers the runs it
/// moved.
async fn carry_on(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    deleted_at: DateTime<Utc>,
    author: Author<'_>,
) -> Result<Vec<Settled>, DieselError> {
    let source_id = meeting_id.to_string();
    let actor = Actor {
        user_id: author.user_id,
        at: author.at,
    };
    let mut carried = Vec::new();
    for name in [workflow::workflow().name(), workflow::alignment().name()] {
        let newest =
            workflow_run::newest(connection, name, workflow::SOURCE_KIND, &source_id).await?;
        let Some(run) = newest.filter(|run| run.state == WorkflowRunState::Failed) else {
            continue;
        };
        let steps = workflow_run::steps_of(connection, run.id).await?;
        let stopped = steps.iter().find(|step| {
            step.state == StepJobState::Failed
                && step
                    .finished_at
                    .is_some_and(|finished_at| finished_at >= deleted_at)
        });
        let Some(stopped) = stopped else {
            // It failed before the deletion, and waits for its person as it did.
            continue;
        };
        match step_job::retry(connection, run.id, stopped.id, actor).await {
            Ok(settled) => carried.push(settled),
            // Retried by someone else meanwhile, so it goes on already.
            Err(StepActionError::Refused(reason)) => event!(
                name: "meetings.restore.run_moved",
                Level::DEBUG,
                meeting.id = %meeting_id,
                workflow_run.id = %run.id,
                reason,
                "a run a deletion stopped was moved on before the restore could retry it",
            ),
            Err(StepActionError::Database(error)) => return Err(error),
        }
    }
    Ok(carried)
}

/// Tells every client what `outcome` changed, once it has committed: each meeting, by its history
/// entry, each segment that left or came back to its timelines, each inbox item a deletion
/// dismissed, and each run restoring carried on, which the engine is told of too. Events carry
/// ids, never content.
pub async fn announce(events: &EventBus, connection: &mut AsyncPgConnection, outcome: Outcome) {
    let mut carried_on = false;
    for changed in outcome.changed {
        events.publish(&ServerEvent::MeetingChanged {
            id: changed.meeting.id,
            state: changed.meeting.state.clone(),
            created_by: changed.meeting.created_by,
            entry_id: Some(changed.entry.id),
        });
        for record in &changed.records {
            events.publish(&ServerEvent::TimelineRecordChanged {
                id: record.id,
                created_by: record.created_by,
            });
        }
        for settled in &changed.runs {
            workflows::announce(events, settled);
            carried_on = true;
        }
        if let Some(item) = changed.dismissed
            && let Err(error) = publish_item_write(events, connection, item, &[], Utc::now()).await
        {
            // The item is dismissed; clients that missed it read it on their next load.
            event!(
                name: "meetings.deletion.unpublished",
                Level::WARN,
                meeting.id = %changed.meeting.id,
                error.message = %error,
                "a deleted meeting's review item was dismissed but could not be announced",
            );
        }
    }
    if carried_on {
        events.signal(&Signal::Workflows);
    }
}
