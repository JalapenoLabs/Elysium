// Copyright © 2026 Jalapeno Labs

//! Meetings that turn out to be one, and the person who decides. See `docs/meetings.md`, The
//! same meeting twice.
//!
//! Teams and Fireflies often record the same meeting, and each listing stores a meeting of its
//! own. Elysium never merges them on its own. When the align step finds that a meeting overlaps
//! another of its person's in time, with participants in common ([`looks_like`]), the later of the
//! two asks: an inbox item, "Same meeting as" the other's title, and a pair
//! (`models::meeting_duplicate`) recording it once, whichever of the two found the other
//! ([`ask_about`]). Its run waits at aligning for the answer, so a duplicate never publishes
//! segments of its own.
//!
//! - **Merge** ([`merge`]) moves its captures into the other meeting, placed on its clock from when
//!   their sources said each started, carries every edit made on it across, builds the other's
//!   transcript again keeping every edit, and starts the other's alignment run, which finds each
//!   moved capture's offset. The merged meeting names the one it went into and is listed nowhere.
//! - **Not the same** ([`not_the_same`]) is remembered, so the pair is never asked about again.
//!
//! Either lets the waiting run go on: a merged meeting's steps skip it. A merge is recorded in both
//! meetings' histories, and undone by splitting ([`split`]): the captures go back where they were,
//! the edits it carried are taken back, both transcripts are built again, and the pair becomes not
//! the same. A split is undone by merging again ([`merge_again`]). Only the meeting's person
//! decides, merges, or splits.

use std::collections::HashSet;

use anyhow::anyhow;
use chrono::{DateTime, Duration, Utc};
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{Level, event};
use uuid::Uuid;

use super::align::AlignMeeting;
use super::final_transcript;
use super::history::UndoError;
use super::workflow;
use crate::action_items::{Actor, Transition, WorkError};
use crate::models::action_item::{
    self, ActionItem, ActionItemPriority, ActionItemState, NewActionItem, Owner,
};
use crate::models::action_item_event::Recorded;
use crate::models::agent::Author;
use crate::models::meeting::{self, Meeting, MeetingState};
use crate::models::meeting_capture::{self, OffsetSource, Placement};
use crate::models::meeting_duplicate::{self, Decision, MeetingDuplicate, NewMeetingDuplicate};
use crate::models::meeting_event::{self, Change, EventKind, MeetingEvent};
use crate::models::meeting_word;
use crate::models::step_job::{self, StepJobState};
use crate::models::user::SYSTEM_USER_ID;
use crate::models::workflow_run::{self, NewRun, Settled};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::routes::v1::action_items::publish_item_write;
use crate::workflows::{self, Step};

/// The longest title of an inbox item, as items allow.
const ITEM_TITLE_MAX_CHARACTERS: usize = 500;

/// Two meetings whose length a source left unknown overlap when they started at most this far
/// apart: about how late a recorder may join, while two meetings booked back to back stay apart.
const START_DRIFT_MINUTES: i64 = 10;

/// The longest a meeting may run and still be compared with one that started this long after it:
/// a full working day, so a meeting is never compared with every meeting of a week.
const LONGEST_MEETING_HOURS: i64 = 12;

/// How many meetings a merge follows to reach the one that holds the captures now, when the other
/// meeting was itself merged since. A chain this long only a fault could build.
const MERGED_CHAIN_MAX: usize = 16;

/// Whether `meeting` and `other` look like one meeting: they overlap in time, and someone took
/// part in both. A meeting whose length its source left unknown overlaps one that started within
/// [`START_DRIFT_MINUTES`] of it.
pub fn looks_like(meeting: &Meeting, other: &Meeting) -> bool {
    let ends = |meeting: &Meeting| {
        meeting.started_at + Duration::milliseconds(meeting.duration_ms.unwrap_or(0).max(0))
    };
    let drift = Duration::minutes(START_DRIFT_MINUTES);
    let close_starts = (meeting.started_at - other.started_at).abs() <= drift;
    let overlap = meeting.started_at < ends(other) && other.started_at < ends(meeting);
    if !close_starts && !overlap {
        return false;
    }
    let taking_part = participants(meeting);
    participants(other)
        .iter()
        .any(|participant| taking_part.contains(participant))
}

/// Who took part in `meeting`, as their names or addresses compare: trimmed and in lowercase.
fn participants(meeting: &Meeting) -> HashSet<String> {
    meeting
        .participants
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|participant| participant.trim().to_lowercase())
        .filter(|participant| !participant.is_empty())
        .collect()
}

/// Whether `meeting` was found after `other`, and so is the one of the two that asks.
fn is_later(meeting: &Meeting, other: &Meeting) -> bool {
    (meeting.created_at, meeting.id) > (other.created_at, other.id)
}

/// What [`ask_about`] found for a meeting.
#[derive(Debug)]
pub struct Asking {
    /// The pair it waits on its person's answer to, if it does.
    pub waiting: Option<MeetingDuplicate>,
    /// The inbox item raised asking, when this call raised it.
    pub raised: Option<Recorded<ActionItem>>,
}

/// The pair meeting `meeting_id` waits on its person's answer to, raising one when it is the later
/// of two meetings that look like one ([`looks_like`]) and no pair of the two was recorded: an
/// inbox item for its person, "Same meeting as" the other's title, raised as the system. Under
/// the meeting's lock, so an attempt that runs again raises nothing more.
///
/// # Errors
/// Propagates any database error, and an item that cannot be raised.
pub async fn ask_about(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> anyhow::Result<Asking> {
    connection
        .transaction(async |connection| {
            let asking = meeting::lock(connection, meeting_id).await?;
            if asking.merged_into.is_some() {
                return Ok(Asking {
                    waiting: None,
                    raised: None,
                });
            }
            let pairs = meeting_duplicate::of_meeting(connection, meeting_id).await?;
            let waiting = pairs
                .iter()
                .find(|pair| pair.meeting_id == meeting_id && pair.decision.is_none());
            if let Some(waiting) = waiting {
                return Ok(Asking {
                    waiting: Some(waiting.clone()),
                    raised: None,
                });
            }
            let paired: HashSet<Uuid> = pairs
                .iter()
                .flat_map(|pair| [pair.meeting_id, pair.same_as_id])
                .collect();
            let window = (
                asking.started_at - Duration::hours(LONGEST_MEETING_HOURS),
                asking.started_at
                    + Duration::milliseconds(asking.duration_ms.unwrap_or(0).max(0))
                    + Duration::minutes(START_DRIFT_MINUTES),
            );
            let nearby =
                meeting::started_between(connection, asking.created_by, meeting_id, window).await?;
            let Some(same_as) = nearby.into_iter().find(|other| {
                !paired.contains(&other.id)
                    && is_later(&asking, other)
                    && looks_like(&asking, other)
            }) else {
                return Ok(Asking {
                    waiting: None,
                    raised: None,
                });
            };
            let pair = NewMeetingDuplicate {
                id: Uuid::now_v7(),
                meeting_id,
                same_as_id: same_as.id,
                action_item_id: None,
                created_by: SYSTEM_USER_ID,
            };
            let Some(pair) = meeting_duplicate::insert_new(connection, pair).await? else {
                // The other meeting recorded the pair first, while this one held no lock of its.
                return Ok(Asking {
                    waiting: None,
                    raised: None,
                });
            };
            let raised = action_item::create(
                connection,
                same_meeting_item(&asking, &same_as),
                Actor::System,
                Utc::now(),
            )
            .await
            .map_err(|error| anyhow!("cannot raise the inbox item asking: {error}"))?;
            let pair = meeting_duplicate::set_item(connection, pair.id, raised.record.id).await?;
            anyhow::Ok(Asking {
                waiting: Some(pair),
                raised: Some(raised),
            })
        })
        .await
}

/// The inbox item asking `asking`'s person whether it is the same meeting as `same_as`. Items are
/// the whole workspace's, so it carries the meetings' titles, as an item a person's mail raises
/// carries its subject, and nothing that was said.
fn same_meeting_item(asking: &Meeting, same_as: &Meeting) -> NewActionItem {
    let other_title = same_as.title.trim();
    let title = if other_title.is_empty() {
        "Same meeting as another".to_owned()
    } else {
        format!("Same meeting as {other_title}")
    };
    let asking_title = asking.title.trim();
    let notes = format!(
        "Two recordings overlap in time with people in common: {} and {}. Merge puts them in one \
         meeting; Not the same keeps them apart.",
        if asking_title.is_empty() {
            "a meeting"
        } else {
            asking_title
        },
        if other_title.is_empty() {
            "another"
        } else {
            other_title
        },
    );
    NewActionItem {
        title: title.chars().take(ITEM_TITLE_MAX_CHARACTERS).collect(),
        notes,
        state: ActionItemState::Inbox,
        priority: ActionItemPriority::Normal,
        due_at: None,
        owner: Owner::User,
        project_ids: Vec::new(),
        initiative_ids: Vec::new(),
    }
}

/// Why a person's answer about a pair could not be taken.
#[derive(Debug, thiserror::Error)]
pub enum DecideError {
    /// There is no such pair, or its meeting is not theirs.
    #[error("not found")]
    NotFound,
    /// It was decided already, or one of its meetings was merged since.
    #[error("the pair was decided already")]
    Decided,
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// What a person's answer about a pair changed, for the caller to announce once it commits.
#[derive(Debug)]
pub struct Decided {
    pub pair: MeetingDuplicate,
    /// The meetings it changed, as they now stand.
    pub meetings: Vec<Meeting>,
    /// The runs it moved: the waiting run let through, and runs it started.
    pub runs: Vec<Settled>,
    /// The inbox item it resolved.
    pub item: Option<Recorded<ActionItem>>,
}

/// Tells every client what `decided` changed, once it has committed: each meeting, each run it
/// moved, and the inbox item it resolved, and the engine that runs are waiting.
pub async fn announce(events: &EventBus, connection: &mut AsyncPgConnection, decided: Decided) {
    for changed in &decided.meetings {
        events.publish(&ServerEvent::MeetingChanged {
            id: changed.id,
            state: changed.state.clone(),
            created_by: changed.created_by,
        });
    }
    for settled in &decided.runs {
        workflows::announce(events, settled);
    }
    if !decided.runs.is_empty() {
        events.signal(&Signal::Workflows);
    }
    if let Some(item) = decided.item
        && let Err(error) = publish_item_write(events, connection, item, &[], Utc::now()).await
    {
        // The item is resolved; clients that missed it read it on their next load.
        event!(
            name: "meetings.duplicates.unpublished",
            Level::WARN,
            meeting_duplicate.id = %decided.pair.id,
            error.message = %error,
            "a pair's inbox item was resolved but could not be announced",
        );
    }
}

/// Merges pair `pair_id`'s meeting into the meeting it looks like, as `person`, its meeting's
/// person: see the module's docs. When the other meeting was merged into a third since, the
/// captures go where it went.
///
/// # Errors
/// [`DecideError::NotFound`] for a pair that is not theirs, [`DecideError::Decided`] for one
/// decided already; any database error.
pub async fn merge(
    connection: &mut AsyncPgConnection,
    pair_id: Uuid,
    person: Uuid,
    now: DateTime<Utc>,
) -> Result<Decided, DecideError> {
    connection
        .transaction(async |connection| {
            let pair = undecided_of(connection, pair_id, person).await?;
            let keeper_id = holder_of(connection, pair.same_as_id).await?;
            let (merging, keeper) = lock_both(connection, pair.meeting_id, keeper_id).await?;
            if merging.merged_into.is_some() || merging.id == keeper.id {
                return Err(DecideError::Decided);
            }
            let author = person_at(person, now);
            merge_captures(connection, &merging, &keeper, author, None).await?;
            let pair =
                meeting_duplicate::decide(connection, pair.id, Decision::Merged, (person, now))
                    .await?;
            let item = resolve_item(connection, &pair, person, now).await?;
            let mut runs = Vec::new();
            runs.extend(let_waiting_run_through(connection, merging.id, person, now).await?);
            runs.extend(start_alignment(connection, &keeper).await?);
            let meetings = vec![
                meeting::find(connection, merging.id).await?,
                meeting::find(connection, keeper.id).await?,
            ];
            Ok(Decided {
                pair,
                meetings,
                runs,
                item,
            })
        })
        .await
}

/// Records that pair `pair_id`'s two meetings are not the same, as `person`, its meeting's person,
/// so it is never asked about again, and lets the meeting's waiting run go on.
///
/// # Errors
/// [`DecideError::NotFound`] for a pair that is not theirs, [`DecideError::Decided`] for one
/// decided already; any database error.
pub async fn not_the_same(
    connection: &mut AsyncPgConnection,
    pair_id: Uuid,
    person: Uuid,
    now: DateTime<Utc>,
) -> Result<Decided, DecideError> {
    connection
        .transaction(async |connection| {
            let pair = undecided_of(connection, pair_id, person).await?;
            let asking = meeting::lock(connection, pair.meeting_id).await?;
            let pair =
                meeting_duplicate::decide(connection, pair.id, Decision::NotTheSame, (person, now))
                    .await?;
            let item = resolve_item(connection, &pair, person, now).await?;
            let runs = let_waiting_run_through(connection, asking.id, person, now)
                .await?
                .into_iter()
                .collect();
            Ok(Decided {
                pair,
                meetings: vec![asking],
                runs,
                item,
            })
        })
        .await
}

/// Pair `pair_id`, when its meeting is `person`'s and it is not decided yet.
async fn undecided_of(
    connection: &mut AsyncPgConnection,
    pair_id: Uuid,
    person: Uuid,
) -> Result<MeetingDuplicate, DecideError> {
    let pair = match meeting_duplicate::find(connection, pair_id).await {
        Ok(pair) => pair,
        Err(diesel::result::Error::NotFound) => return Err(DecideError::NotFound),
        Err(error) => return Err(error.into()),
    };
    match meeting::find_owned(connection, pair.meeting_id, person).await {
        Ok(_asking) => {}
        Err(diesel::result::Error::NotFound) => return Err(DecideError::NotFound),
        Err(error) => return Err(error.into()),
    }
    if pair.decision.is_some() {
        return Err(DecideError::Decided);
    }
    Ok(pair)
}

/// The meeting that holds meeting `meeting_id`'s captures now: itself, or the one it was merged
/// into, followed as far as it was merged on.
async fn holder_of(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> Result<Uuid, DecideError> {
    let mut holder = meeting::find(connection, meeting_id).await?;
    for _step in 0..MERGED_CHAIN_MAX {
        let Some(into) = holder.merged_into else {
            return Ok(holder.id);
        };
        holder = meeting::find(connection, into).await?;
    }
    event!(
        name: "meetings.duplicates.chain",
        Level::WARN,
        meeting.id = %meeting_id,
        "a meeting's merges run on past what a person could have made; refusing to merge into it",
    );
    Err(DecideError::Decided)
}

/// Locks meetings `first` and `second`, in the order of their ids, so two writers locking the same
/// two never wait on each other, and answers them in the order asked.
async fn lock_both(
    connection: &mut AsyncPgConnection,
    first: Uuid,
    second: Uuid,
) -> diesel::QueryResult<(Meeting, Meeting)> {
    if first == second {
        let only = meeting::lock(connection, first).await?;
        return Ok((only.clone(), only));
    }
    if first < second {
        let first = meeting::lock(connection, first).await?;
        let second = meeting::lock(connection, second).await?;
        return Ok((first, second));
    }
    let second = meeting::lock(connection, second).await?;
    let first = meeting::lock(connection, first).await?;
    Ok((first, second))
}

/// `person`, acting alone at `at`.
const fn person_at(person: Uuid, at: DateTime<Utc>) -> Author<'static> {
    Author {
        user_id: person,
        agent: None,
        at,
    }
}

/// What a merge or a split moved, as both meetings' histories record it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Moved {
    /// The meeting the captures came from, and the one they went into.
    pub from: Uuid,
    pub into: Uuid,
    /// Each capture moved, where it sat before and after.
    pub captures: Vec<MovedCapture>,
}

/// One capture a merge moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovedCapture {
    pub capture_id: Uuid,
    pub before: Placement,
    pub after: Placement,
}

/// Moves every capture of `merging` into `keeper`, as `author`, and records it in both histories,
/// the keeper's entry undoing `undid` when the merge is a split's undo. Each capture is placed on
/// the keeper's clock from when the two meetings started, after the keeper's own captures; the
/// merging's edits are carried across; `merging` names `keeper` as where it went; and the keeper's
/// transcript is built again. Answers the keeper's entry. Both meetings are locked by the caller.
async fn merge_captures(
    connection: &mut AsyncPgConnection,
    merging: &Meeting,
    keeper: &Meeting,
    author: Author<'_>,
    undid: Option<Uuid>,
) -> anyhow::Result<MeetingEvent> {
    let captures = meeting_capture::list_of(connection, merging.id).await?;
    let shift_ms = (merging.started_at - keeper.started_at).num_milliseconds();
    let moved = Moved {
        from: merging.id,
        into: keeper.id,
        captures: captures
            .iter()
            .map(|capture| MovedCapture {
                capture_id: capture.id,
                before: capture.placement(),
                after: Placement {
                    meeting_id: keeper.id,
                    joined_at: author.at,
                    offset_ms: capture.offset_ms + shift_ms,
                    offset_source: OffsetSource::Listed,
                },
            })
            .collect(),
    };
    let entry = record_moved(
        connection,
        (keeper.id, merging.id),
        EventKind::Merged,
        &moved,
        author,
        undid,
    )
    .await?;
    for capture in &moved.captures {
        meeting_capture::place(connection, capture.capture_id, capture.after).await?;
    }
    meeting_word::carry_edits(connection, (merging.id, keeper.id), shift_ms, entry.id).await?;
    meeting::set_merged_into(connection, merging.id, Some(keeper.id)).await?;
    final_transcript::rebuild(connection, keeper.id, author).await?;
    Ok(entry)
}

/// Records `moved` as a change of `kind` in both meetings' histories, `(keeper, merging)`, the
/// keeper's entry naming the change it undid, if any. Answers the keeper's entry, which is the one
/// undone.
async fn record_moved(
    connection: &mut AsyncPgConnection,
    (keeper_id, merging_id): (Uuid, Uuid),
    kind: EventKind,
    moved: &Moved,
    author: Author<'_>,
    undid: Option<Uuid>,
) -> diesel::QueryResult<MeetingEvent> {
    let data = serde_json::to_value(moved).expect("what a merge moved is plain fields");
    let entry = meeting_event::record(
        connection,
        Change {
            id: Uuid::now_v7(),
            meeting_id: keeper_id,
            kind,
            author,
            data: data.clone(),
            undid,
        },
    )
    .await?;
    meeting_event::record(
        connection,
        Change {
            id: Uuid::now_v7(),
            meeting_id: merging_id,
            kind,
            author,
            data,
            undid: None,
        },
    )
    .await?;
    Ok(entry)
}

/// Splits back out what `undone`, a `merged` entry of `keeper`'s, merged in, as `author`: each
/// capture goes back where it was, the edits it carried are taken back, the merged meeting is its
/// own again, both transcripts are built again, every pair of the two becomes not the same, and
/// runs start that align both again. `keeper` is locked by the caller.
///
/// # Errors
/// [`UndoError::NotUndoable`] for the merged meeting's own entry, which its keeper's undoes;
/// [`UndoError::ChangedSince`] once a capture moved on since; any database error.
pub(crate) async fn split(
    connection: &mut AsyncPgConnection,
    keeper: &Meeting,
    undone: &MeetingEvent,
    author: Author<'_>,
) -> Result<MeetingEvent, UndoError> {
    let moved = moved_of(undone)?;
    let merging = meeting::lock(connection, moved.from).await?;
    if merging.merged_into != Some(keeper.id) {
        return Err(UndoError::ChangedSince);
    }
    if !captures_sit(connection, &moved, keeper.id, |capture| capture.after).await? {
        return Err(UndoError::ChangedSince);
    }
    let entry = record_moved(
        connection,
        (keeper.id, merging.id),
        EventKind::Split,
        &moved,
        author,
        Some(undone.id),
    )
    .await?;
    for capture in &moved.captures {
        meeting_capture::place(connection, capture.capture_id, capture.before).await?;
    }
    meeting_word::withdraw_carried(connection, keeper.id, undone.id, entry.id).await?;
    let merging = meeting::set_merged_into(connection, merging.id, None).await?;
    decide_pairs(
        connection,
        (keeper.id, merging.id),
        Decision::NotTheSame,
        author,
    )
    .await?;
    final_transcript::rebuild(connection, keeper.id, author)
        .await
        .map_err(UndoError::Transcript)?;
    final_transcript::rebuild(connection, merging.id, author)
        .await
        .map_err(UndoError::Transcript)?;
    start_alignment(connection, keeper)
        .await
        .map_err(UndoError::Transcript)?;
    if merging.state() == MeetingState::Done {
        start_alignment(connection, &merging)
            .await
            .map_err(UndoError::Transcript)?;
    } else {
        // Its run went through skipping it while it was merged; it is processed from aligning on.
        let carried_on = workflow::run_from_aligning(connection, &merging)
            .await
            .map_err(UndoError::Transcript)?;
        workflow_run::start(connection, carried_on, author.at).await?;
    }
    Ok(entry)
}

/// Merges again what `undone`, a `split` entry of `keeper`'s, split back out, as `author`.
///
/// # Errors
/// [`UndoError::NotUndoable`] for the other meeting's own entry; [`UndoError::ChangedSince`] once
/// a capture moved on since; any database error.
pub(crate) async fn merge_again(
    connection: &mut AsyncPgConnection,
    keeper: &Meeting,
    undone: &MeetingEvent,
    author: Author<'_>,
) -> Result<MeetingEvent, UndoError> {
    let moved = moved_of(undone)?;
    let merging = meeting::lock(connection, moved.from).await?;
    if merging.merged_into.is_some() || keeper.merged_into.is_some() {
        return Err(UndoError::ChangedSince);
    }
    if !captures_sit(connection, &moved, merging.id, |capture| capture.before).await? {
        return Err(UndoError::ChangedSince);
    }
    let entry = merge_captures(connection, &merging, keeper, author, Some(undone.id))
        .await
        .map_err(UndoError::Transcript)?;
    decide_pairs(
        connection,
        (keeper.id, merging.id),
        Decision::Merged,
        author,
    )
    .await?;
    start_alignment(connection, keeper)
        .await
        .map_err(UndoError::Transcript)?;
    Ok(entry)
}

/// What `undone` moved, when it is the keeper's entry of a merge or a split.
fn moved_of(undone: &MeetingEvent) -> Result<Moved, UndoError> {
    let moved: Moved = serde_json::from_value(undone.data.clone()).map_err(|error| {
        event!(
            name: "meetings.history.unreadable",
            Level::WARN,
            meeting_event.id = %undone.id,
            error.message = %error,
            "a merge's history entry is not in its shape; it cannot be undone",
        );
        UndoError::NotUndoable
    })?;
    // The merged meeting's own entry mirrors the keeper's; the keeper's is the one undone.
    if moved.into != undone.meeting_id {
        return Err(UndoError::NotUndoable);
    }
    Ok(moved)
}

/// Whether every capture `moved` moved still sits in meeting `meeting_id` where `placed` says.
async fn captures_sit(
    connection: &mut AsyncPgConnection,
    moved: &Moved,
    meeting_id: Uuid,
    placed: impl Fn(&MovedCapture) -> Placement,
) -> diesel::QueryResult<bool> {
    let present: HashSet<Uuid> = meeting_capture::list_of(connection, meeting_id)
        .await?
        .into_iter()
        .map(|capture| capture.id)
        .collect();
    Ok(moved.captures.iter().all(|capture| {
        placed(capture).meeting_id == meeting_id && present.contains(&capture.capture_id)
    }))
}

/// Records `decision` on every pair of the two meetings `between`, as `author`.
async fn decide_pairs(
    connection: &mut AsyncPgConnection,
    (first, second): (Uuid, Uuid),
    decision: Decision,
    author: Author<'_>,
) -> diesel::QueryResult<()> {
    for pair in meeting_duplicate::of_meeting(connection, first).await? {
        if pair.meeting_id == second || pair.same_as_id == second {
            meeting_duplicate::decide(connection, pair.id, decision, (author.user_id, author.at))
                .await?;
        }
    }
    Ok(())
}

/// Resolves the inbox item `pair` asked with, as `person`, unless it is resolved, dismissed, or
/// deleted already: the person may have dealt with it from Action items first.
async fn resolve_item(
    connection: &mut AsyncPgConnection,
    pair: &MeetingDuplicate,
    person: Uuid,
    now: DateTime<Utc>,
) -> diesel::QueryResult<Option<Recorded<ActionItem>>> {
    let Some(item_id) = pair.action_item_id else {
        return Ok(None);
    };
    match action_item::transition(
        connection,
        item_id,
        Transition::Resolve,
        Actor::User(person),
        now,
    )
    .await
    {
        Ok(resolved) => Ok(Some(resolved)),
        Err(WorkError::Database(diesel::result::Error::NotFound)) => Ok(None),
        Err(WorkError::Database(error)) => Err(error),
        // Already resolved or dismissed, or deleted: there is nothing left to resolve.
        Err(_refused) => Ok(None),
    }
}

/// Lets meeting `meeting_id`'s run through when its align step waits on the person's answer, as
/// `person`, with the output it waited with: a merged meeting's steps then skip it, and one that
/// is not the same goes on.
async fn let_waiting_run_through(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    person: Uuid,
    now: DateTime<Utc>,
) -> Result<Option<Settled>, DecideError> {
    let live = workflow_run::find_live(
        connection,
        workflow::workflow().name(),
        workflow::SOURCE_KIND,
        &meeting_id.to_string(),
    )
    .await?;
    let Some(run) = live else {
        return Ok(None);
    };
    let steps = workflow_run::steps_of(connection, run.id).await?;
    let Some(held) = steps
        .into_iter()
        .find(|step| step.step == AlignMeeting::NAME && step.state == StepJobState::Held)
    else {
        return Ok(None);
    };
    let actor = workflow_run::Actor {
        user_id: person,
        at: now,
    };
    let settled = step_job::resume(connection, run.id, held.id, actor)
        .await
        .map_err(|error| anyhow!("cannot let the waiting run through: {error}"))?;
    Ok(Some(settled))
}

/// Starts a run of `meeting`'s alignment (`workflow::alignment`), which finds where each of its
/// captures sits on its clock and lines its transcript up again, unless one is under way already.
async fn start_alignment(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
) -> anyhow::Result<Option<Settled>> {
    let new_run: NewRun = workflow::run_alignment(connection, meeting).await?;
    let started = workflow_run::start(connection, new_run, Utc::now()).await?;
    Ok(started.created.then_some(started.settled))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn meeting(started_at: &str, duration_minutes: Option<i64>, participants: &[&str]) -> Meeting {
        Meeting {
            id: Uuid::now_v7(),
            title: "Weekly sync".to_owned(),
            started_at: started_at.parse().expect("a time"),
            duration_ms: duration_minutes.map(|minutes| minutes * 60_000),
            organizer: String::new(),
            participants: json!(participants),
            segments: None,
            state: "recorded".to_owned(),
            created_by: Uuid::now_v7(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            hold_for_approval: false,
            merged_into: None,
        }
    }

    #[test]
    fn meetings_that_overlap_with_someone_in_both_look_like_one() {
        let teams = meeting(
            "2026-11-10T15:00:00Z",
            Some(60),
            &["ada@example.com", "Grace"],
        );
        // Fireflies joined four minutes late and names Ada in another case.
        let fireflies = meeting("2026-11-10T15:04:00Z", Some(55), &[" ADA@example.com "]);
        assert!(looks_like(&teams, &fireflies));
        assert!(looks_like(&fireflies, &teams));

        // A source that kept no length still matches one starting within ten minutes.
        let unknown_length = meeting("2026-11-10T15:08:00Z", None, &["grace"]);
        assert!(looks_like(&teams, &unknown_length));
    }

    #[test]
    fn meetings_apart_in_time_or_with_nobody_in_common_do_not() {
        let standup = meeting("2026-11-10T15:00:00Z", Some(30), &["ada@example.com"]);
        let next = meeting("2026-11-10T15:30:00Z", Some(30), &["ada@example.com"]);
        assert!(!looks_like(&standup, &next), "back to back is two meetings");

        let other_people = meeting("2026-11-10T15:00:00Z", Some(30), &["linus@example.com"]);
        assert!(!looks_like(&standup, &other_people));

        let nobody = meeting("2026-11-10T15:00:00Z", Some(30), &[]);
        assert!(!looks_like(&standup, &nobody));
    }
}
