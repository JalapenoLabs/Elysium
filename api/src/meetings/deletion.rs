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
//! - **Its excerpts' records** go with it, stamped with the meeting's own `deleted_at`
//!   (`timeline_record::delete_of_meeting`): off every project's timeline, out of every Project
//!   Manager's reading, and out of the knowledge index, each keeping its attributions and their
//!   history. So does its whole transcript's own document in the index
//!   (`excerpts::index_transcript`). Its marks stay as they are, a removed excerpt removed.
//!   Restoring brings back exactly the records that stamp names and the transcript's document,
//!   and indexes each again as it reads now.
//! - **Its run stops.** Every meeting step fails for good on a deleted meeting with [`STOPPED`], at
//!   its start and again under the meeting's lock where it would settle words, raise an inbox item,
//!   or take its excerpts, so a deleted meeting asks nothing of its person, puts nothing on a
//!   timeline, and indexes nothing. The inbox items it raised already, asking its person something
//!   about it (to approve its corrected transcript, and whether to open it once its correction kept
//!   it private), are dismissed as whoever deletes it ([`dismiss_asks`]), the `deleted` entry naming
//!   each and where it stood, and restoring returns each still dismissed there, the inbox or open
//!   ([`return_asks`]), so a connector's removal that fails and restores what it deleted leaves the
//!   asks as they were too. Restoring retries the step that
//!   stopped: the failed step of the meeting's newest run of each meeting workflow that failed
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
//! Deleting and restoring take what managing a meeting takes, its owner or an admin
//! (`super::access`, [`Need::Manage`]), as `docs/meeting-review.md`, Who sees a meeting, has it: a
//! meeting is opened as every meeting route opens it, then locked, so a deletion waits for an
//! edit, a merge, a step's publishing, or an upload's finish, and they for it. A meeting merged
//! into another is refused (`meeting_merged`): its captures live on in the meeting kept, which is
//! the one to delete. A meeting whose connector was removed is deleted like any other.
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

use super::access::{self, Need};
use super::excerpts::{self, MeetingText};
use super::workflow;
use crate::action_items::{Actor as ItemActor, Transition, WorkError};
use crate::models::action_item::{self, ActionItem, ActionItemState};
use crate::models::action_item_event::Recorded;
use crate::models::agent::Author;
use crate::models::knowledge_document::{self, KnowledgeSource};
use crate::models::meeting::{self, Meeting};
use crate::models::meeting_correction;
use crate::models::meeting_event::{self, Change, EventKind, MeetingEvent};
use crate::models::step_job::{self, StepActionError, StepJobState};
use crate::models::timeline_record::{self, TimelineRecord};
use crate::models::user::User;
use crate::models::workflow_run::{self, Actor, Settled, WorkflowRunState};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::routes::v1::action_items::publish_item_writes;
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
    /// No meeting they may read has this id.
    NotFound,
    /// They read it, and only its owner or an admin deletes or restores it.
    OwnerOrAdmin,
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
            Self::OwnerOrAdmin => "owner_or_admin",
            Self::Merged => "meeting_merged",
            Self::Deleted => "meeting_deleted",
            Self::NotDeleted => "meeting_not_deleted",
        }
    }

    /// Why, in words for the person.
    pub const fn message(self) -> &'static str {
        match self {
            Self::NotFound => "no meeting you may read has this id",
            Self::OwnerOrAdmin => "only its owner or an admin deletes or restores a meeting",
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
    /// The excerpts' records that went with it, or came back.
    pub records: Vec<TimelineRecord>,
    /// The runs restoring it carried on.
    pub runs: Vec<Settled>,
    /// The inbox items asking its person something about it, which deleting it dismissed or
    /// restoring it returned to where they stood.
    pub items: Vec<Recorded<ActionItem>>,
}

/// An inbox item a deletion dismissed, and where it stood: the inbox, where it was raised, or open,
/// once its person accepted it. The `deleted` entry records each (`dismissedItems`), and restoring
/// returns each there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DismissedItem {
    id: Uuid,
    from: ActionItemState,
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
/// `at`, each one they may manage. Each is changed once, in the order of their ids, so two changes
/// over the same meetings lock them in one order. `removed_with` names the connector the meetings are deleted with, which
/// their history records. Nothing is announced: the caller does, once it commits ([`announce`]).
///
/// # Errors
/// Propagates any database error, which writes nothing. A meeting refused is reported in
/// [`Outcome::refused`].
pub async fn change_many(
    connection: &mut AsyncPgConnection,
    meeting_ids: &[Uuid],
    change: BulkChange,
    person: &User,
    at: DateTime<Utc>,
    removed_with: Option<Uuid>,
) -> Result<Outcome, DieselError> {
    let author = Author {
        user_id: person.id,
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
                    .transaction(async move |connection| {
                        let locked = lock_changeable(connection, meeting_id, person).await?;
                        match change {
                            BulkChange::Delete {} => {
                                delete_one(connection, &locked, author, removed_with).await
                            }
                            BulkChange::Restore {} => {
                                restore_one(connection, &locked, author).await
                            }
                        }
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

/// Meeting `meeting_id` locked for the change, when `person` may manage it and it holds its own
/// captures, or why it refuses: one they may not read, one they read but may not manage, or one
/// merged into another.
async fn lock_changeable(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    person: &User,
) -> Result<Meeting, Failure> {
    // Opened as every meeting route opens it, so who may change it is decided in one place.
    match access::open(connection, meeting_id, person, Need::Manage).await {
        Ok(_) => {}
        Err(access::Refusal::NotFound) => return Err(Failure::Refused(RefusalKind::NotFound)),
        Err(access::Refusal::Needs(_)) => return Err(Failure::Refused(RefusalKind::OwnerOrAdmin)),
        Err(access::Refusal::Database(error)) => return Err(error.into()),
    }
    // The lock every change to a meeting takes, read again under it.
    let locked = meeting::lock(connection, meeting_id).await?;
    if locked.merged_into.is_some() {
        return Err(Failure::Refused(RefusalKind::Merged));
    }
    Ok(locked)
}

/// Deletes `locked`, the meeting [`lock_changeable`] locked, with its excerpts' records and its
/// transcript's document, as `author`, recording it.
async fn delete_one(
    connection: &mut AsyncPgConnection,
    locked: &Meeting,
    author: Author<'_>,
    removed_with: Option<Uuid>,
) -> Result<Changed, Failure> {
    let meeting_id = locked.id;
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
    knowledge_document::remove(connection, &KnowledgeSource::Meeting { id: meeting_id }).await?;
    // Nothing asks its person about what was deleted; a run held for its approval stays held. The
    // entry names each item and where it stood, so restoring returns exactly the ones this deletion
    // dismissed, to the inbox they were raised in or to open.
    let (dismissed, items) = dismiss_asks(connection, locked, author).await?;
    let entry = meeting_event::record(
        connection,
        Change {
            id: Uuid::now_v7(),
            meeting_id,
            kind: EventKind::Deleted,
            author,
            data: json!({
                "excerpts": records.len(),
                "connectorId": removed_with,
                "dismissedItems": dismissed,
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
        items,
    })
}

/// Restores `locked`, the meeting [`lock_changeable`] locked, with the excerpts' records and the
/// transcript document its deletion took, as `author`, carries on the run the deletion stopped, and
/// records it.
async fn restore_one(
    connection: &mut AsyncPgConnection,
    locked: &Meeting,
    author: Author<'_>,
) -> Result<Changed, Failure> {
    let meeting_id = locked.id;
    let Some(deleted_at) = locked.deleted_at else {
        return Err(Failure::Refused(RefusalKind::NotDeleted));
    };
    let restored = meeting::set_deleted(connection, meeting_id, None).await?;
    let records = timeline_record::restore_of_meeting(connection, meeting_id, deleted_at).await?;
    // Indexed as it reads now, which is as it read when it was deleted, since nothing changed it.
    let text = MeetingText::load(connection, meeting_id).await?;
    excerpts::index_transcript(connection, &restored, &text).await?;
    let runs = carry_on(connection, meeting_id, deleted_at, author).await?;
    let dismissed = dismissed_by_deletion(connection, meeting_id).await?;
    let returned = return_asks(connection, &dismissed, author).await?;
    let entry = meeting_event::record(
        connection,
        Change {
            id: Uuid::now_v7(),
            meeting_id,
            kind: EventKind::Restored,
            author,
            data: json!({
                "deletedAt": deleted_at,
                "excerpts": records.len(),
                "runRetried": !runs.is_empty(),
                "returnedItemIds": returned.iter().map(|item| item.record.id).collect::<Vec<_>>(),
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
        items: returned,
    })
}

/// The inbox items asking `meeting`'s person something about it that are still waiting for them:
/// its correction's review while the corrected copy waits for approval, and the item saying its
/// correction kept it private. One already resolved or dismissed is left as its person left it.
async fn asks_of(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
) -> Result<Vec<ActionItem>, DieselError> {
    let review = meeting_correction::find(connection, meeting.id)
        .await?
        .filter(|correction| correction.approved_at.is_none())
        .and_then(|correction| correction.review_item_id);
    let mut asks = Vec::new();
    for item_id in review.into_iter().chain(meeting.visibility_item_id) {
        match action_item::find(connection, item_id).await {
            Ok(item)
                if item.deleted_at.is_none()
                    && matches!(item.state, ActionItemState::Inbox | ActionItemState::Open) =>
            {
                asks.push(item);
            }
            Ok(_) | Err(DieselError::NotFound) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(asks)
}

/// Dismisses, as `author`, every item [`asks_of`] finds for `locked`, the meeting being deleted,
/// under the lock every change to it takes: nothing asks its person about a meeting that is
/// deleted. Answers each dismissed with where it stood, for the `deleted` entry, and as written.
async fn dismiss_asks(
    connection: &mut AsyncPgConnection,
    locked: &Meeting,
    author: Author<'_>,
) -> Result<(Vec<DismissedItem>, Vec<Recorded<ActionItem>>), DieselError> {
    let mut dismissed = Vec::new();
    let mut written = Vec::new();
    let actor = ItemActor::User(author.user_id);
    for ask in asks_of(connection, locked).await? {
        match action_item::transition(connection, ask.id, Transition::Dismiss, actor, author.at)
            .await
        {
            Ok(item) => {
                dismissed.push(DismissedItem {
                    id: ask.id,
                    from: ask.state,
                });
                written.push(item);
            }
            Err(WorkError::Database(error)) => return Err(error),
            // Changed in the instant since it was read: it stays as that change left it.
            Err(_refused) => {}
        }
    }
    Ok((dismissed, written))
}

/// Returns, as `author`, each of `dismissed` that is still dismissed to where it stood, the inbox
/// or open, as a changeset's undo returns an item (`action_item::return_to`). Nothing changes a
/// deleted meeting, so what each asked still waits; one its person dealt with by hand meanwhile is
/// theirs, and stays as they left it. Answers each returned.
async fn return_asks(
    connection: &mut AsyncPgConnection,
    dismissed: &[DismissedItem],
    author: Author<'_>,
) -> Result<Vec<Recorded<ActionItem>>, DieselError> {
    let mut returned = Vec::new();
    let actor = ItemActor::User(author.user_id);
    for ask in dismissed {
        match action_item::find(connection, ask.id).await {
            Ok(item) if item.state == ActionItemState::Dismissed && item.deleted_at.is_none() => {}
            Ok(_) | Err(DieselError::NotFound) => continue,
            Err(error) => return Err(error),
        }
        match action_item::return_to(connection, ask.id, ask.from, actor, author.at).await {
            Ok(item) => returned.push(item),
            Err(WorkError::Database(error)) => return Err(error),
            // Changed in the instant since it was read: it stays as that change left it.
            Err(_refused) => {}
        }
    }
    Ok(returned)
}

/// The items meeting `meeting_id`'s newest deletion dismissed, as its `deleted` entry names them
/// (`dismissedItems`). An entry whose list does not read is logged and returns nothing.
async fn dismissed_by_deletion(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> Result<Vec<DismissedItem>, DieselError> {
    let deletions = meeting_event::of_kind(connection, meeting_id, EventKind::Deleted).await?;
    let Some(deletion) = deletions.last() else {
        return Ok(Vec::new());
    };
    let listed = deletion
        .data
        .get("dismissedItems")
        .cloned()
        .unwrap_or_else(|| json!([]));
    match serde_json::from_value(listed) {
        Ok(dismissed) => Ok(dismissed),
        Err(error) => {
            // Every deletion writes the list; one that does not read was written by hand or by a
            // broken version, and leaves its items as they are.
            event!(
                name: "meetings.deletion.unreadable",
                Level::WARN,
                meeting.id = %meeting_id,
                meeting_event.id = %deletion.id,
                error.message = %error,
                "a deletion's dismissed items do not read; none is returned",
            );
            Ok(Vec::new())
        }
    }
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

/// Tells every client what `changed` changed, once it has committed: each meeting, by its history
/// entry, each excerpt's record that left or came back to its timelines, each run restoring
/// carried on, which the engine is told of too, and every review item dismissed or returned,
/// published together. Events carry ids, never content. Answers the meetings as they now stand,
/// in order, for a caller to answer with.
pub async fn announce(
    events: &EventBus,
    connection: &mut AsyncPgConnection,
    changed: Vec<Changed>,
) -> Vec<Meeting> {
    let mut carried_on = false;
    let mut items = Vec::new();
    let mut meetings = Vec::with_capacity(changed.len());
    for changed in changed {
        events.publish(&ServerEvent::MeetingChanged {
            id: changed.meeting.id,
            state: changed.meeting.state.clone(),
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
        items.extend(changed.items);
        meetings.push(changed.meeting);
    }
    if carried_on {
        events.signal(&Signal::Workflows);
    }
    if items.is_empty() {
        return meetings;
    }
    let item_ids: Vec<Uuid> = items.iter().map(|item| item.record.id).collect();
    if let Err(error) = publish_item_writes(events, connection, items, &[], Utc::now()).await {
        // The items are written; clients that missed them read them on their next load.
        event!(
            name: "meetings.deletion.unpublished",
            Level::WARN,
            action_item.ids = ?item_ids,
            error.message = %error,
            "review items a deletion or a restore changed could not be announced",
        );
    }
    meetings
}
