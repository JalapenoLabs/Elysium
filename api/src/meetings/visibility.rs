// Copyright © 2026 Jalapeno Labs

//! Who sees a meeting: its owner or an admin making it private, opening it, or sharing it, and its
//! correction deciding for a new one. See `docs/meeting-review.md`, Who sees a meeting.
//!
//! Every change takes the meeting's lock and is in its history in the same transaction:
//! `visibility_changed` with what it was and became, `shared` and `unshared` with the person. A
//! person's change is undone from there while nothing changed it since; the correction's decision
//! is not, since nothing would decide again, and its person changes it from the meeting's options
//! instead.
//!
//! Nothing here touches the knowledge index: a private meeting's segments are left out of a
//! search as it runs, by who may read the meeting then (`models::meeting::hidden_from`), so a
//! change applies to the next search at once.
//!
//! # The correction's decision
//!
//! A new meeting is [`Visibility::Unchecked`]: private to its owner, and found by nobody else.
//! Its correction reads it and judges whether it is sensitive ([`Judgment`]), and [`decide`]
//! applies that at once, in the transaction that settles its transcript: open to every member,
//! or private, shared with the people the correction chose, with an inbox item telling its owner
//! so. A judgment it could not make keeps the meeting private and says that instead, so a
//! meeting is never opened on a guess. A meeting a person already made private or opened is left
//! as they set it.

use chrono::Utc;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde_json::{Value, json};
use uuid::Uuid;

use super::history::UndoError;
use crate::action_items::Actor;
use crate::models::action_item::{
    self, ActionItem, ActionItemPriority, ActionItemState, NewActionItem, Owner,
};
use crate::models::action_item_event::Recorded;
use crate::models::agent::Author;
use crate::models::meeting::{self, Meeting, Visibility};
use crate::models::meeting_event::{self, Change, EventKind, MeetingEvent};
use crate::models::meeting_share;
use crate::models::user::{self, SYSTEM_USER_ID};

/// The longest reason the correction gives for keeping a meeting private, in characters: one
/// sentence, which its owner reads on the meeting's page.
pub const WHY_MAX_CHARACTERS: usize = 300;

/// Why a change to who sees a meeting was refused.
#[derive(Debug, thiserror::Error)]
pub enum VisibilityError {
    /// The meeting does not exist.
    #[error("not found")]
    NotFound,
    /// It is so already: of that visibility, shared with them, or not shared with them.
    #[error("the meeting is so already")]
    Unchanged,
    /// Nobody a meeting can be shared with: not an active person, or its owner, who reads it
    /// anyway.
    #[error("the meeting cannot be shared with them")]
    NotShareable,
    #[error(transparent)]
    Database(diesel::result::Error),
}

impl From<diesel::result::Error> for VisibilityError {
    fn from(error: diesel::result::Error) -> Self {
        match error {
            diesel::result::Error::NotFound => Self::NotFound,
            other => Self::Database(other),
        }
    }
}

/// A change to who sees a meeting, made: the meeting as it now is, and its history entry.
#[derive(Debug)]
pub struct Changed {
    pub meeting: Meeting,
    pub entry: MeetingEvent,
}

/// Makes meeting `meeting_id` private or opens it to every member, as `author`, its owner or an
/// admin, whom the caller checked. Once a person chooses, its correction leaves it as they chose.
///
/// # Errors
/// [`VisibilityError::NotFound`] for no such meeting, [`VisibilityError::Unchanged`] when it is of
/// that visibility already, and any database error.
pub async fn set(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    visibility: Visibility,
    author: Author<'_>,
) -> Result<Changed, VisibilityError> {
    connection
        .transaction(async |connection| {
            let current = meeting::lock(connection, meeting_id).await?;
            let before = current.visibility();
            if before == visibility {
                return Err(VisibilityError::Unchanged);
            }
            let changed = meeting::set_visibility(connection, meeting_id, visibility).await?;
            let entry = record(
                connection,
                meeting_id,
                EventKind::VisibilityChanged,
                json!({ "before": before.as_str(), "after": visibility.as_str() }),
                author,
                None,
            )
            .await?;
            Ok(Changed {
                meeting: changed,
                entry,
            })
        })
        .await
}

/// Shares meeting `meeting_id` with `person_id`, as `author`, its owner or an admin, whom the
/// caller checked.
///
/// # Errors
/// [`VisibilityError::NotShareable`] for anyone but an active person other than its owner,
/// [`VisibilityError::Unchanged`] when it is shared with them already, and any database error.
pub async fn share(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    person_id: Uuid,
    author: Author<'_>,
) -> Result<Changed, VisibilityError> {
    connection
        .transaction(async |connection| {
            let current = meeting::lock(connection, meeting_id).await?;
            if !is_shareable(connection, &current, person_id).await? {
                return Err(VisibilityError::NotShareable);
            }
            if !meeting_share::add(connection, meeting_id, person_id, author.user_id).await? {
                return Err(VisibilityError::Unchanged);
            }
            let entry = record(
                connection,
                meeting_id,
                EventKind::Shared,
                json!({ "personId": person_id }),
                author,
                None,
            )
            .await?;
            Ok(Changed {
                meeting: current,
                entry,
            })
        })
        .await
}

/// Stops sharing meeting `meeting_id` with `person_id`, as `author`, its owner or an admin, whom
/// the caller checked.
///
/// # Errors
/// [`VisibilityError::Unchanged`] when it is not shared with them, and any database error.
pub async fn unshare(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    person_id: Uuid,
    author: Author<'_>,
) -> Result<Changed, VisibilityError> {
    connection
        .transaction(async |connection| {
            let current = meeting::lock(connection, meeting_id).await?;
            if !meeting_share::remove(connection, meeting_id, person_id).await? {
                return Err(VisibilityError::Unchanged);
            }
            let entry = record(
                connection,
                meeting_id,
                EventKind::Unshared,
                json!({ "personId": person_id }),
                author,
                None,
            )
            .await?;
            Ok(Changed {
                meeting: current,
                entry,
            })
        })
        .await
}

/// Whether `current` may be shared with `person_id`: an active person, never a machine, and not
/// its owner.
async fn is_shareable(
    connection: &mut AsyncPgConnection,
    current: &Meeting,
    person_id: Uuid,
) -> diesel::QueryResult<bool> {
    if person_id == current.created_by {
        return Ok(false);
    }
    match user::find(connection, person_id).await {
        Ok(person) => Ok(person.is_active() && !person.is_machine()),
        Err(diesel::result::Error::NotFound) => Ok(false),
        Err(error) => Err(error),
    }
}

/// What a meeting's correction judged of who should see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Judgment {
    /// Nothing in it is sensitive: every member reads it.
    Open,
    /// It is sensitive, such as a one-to-one or a performance review: read by its owner and
    /// `share_with`, the people the correction judged should see it, and why in a sentence.
    Private {
        share_with: Vec<Uuid>,
        why: Option<String>,
    },
    /// Its answer said nothing that could be read about it, so it stays private to its owner.
    Undecided,
}

/// What [`decide`] did.
#[derive(Debug)]
pub struct Decided {
    /// The meeting as it now is.
    pub meeting: Meeting,
    /// The inbox item telling its owner it was kept private, for the caller to announce once the
    /// transaction commits.
    pub raised: Option<Recorded<ActionItem>>,
}

/// Applies `judgment` to `current`, its correction's, as the system, unless a person already made
/// it private or opened it; `None` then, and it stays as they set it. Kept private, it is shared
/// with every person the judgment named who may be, and its owner is told so with an inbox item.
///
/// Call it inside the transaction that settles the meeting, holding its lock, so its transcript
/// and who reads it are decided together.
///
/// # Errors
/// Propagates any database error, and an inbox item that cannot be raised.
pub async fn decide(
    connection: &mut AsyncPgConnection,
    current: &Meeting,
    judgment: &Judgment,
) -> anyhow::Result<Option<Decided>> {
    if current.visibility() != Visibility::Unchecked {
        return Ok(None);
    }
    let author = Author {
        user_id: SYSTEM_USER_ID,
        agent: None,
        at: Utc::now(),
    };
    let (after, why, share_with) = match judgment {
        Judgment::Open => (Visibility::Public, None, Vec::new()),
        Judgment::Private { share_with, why } => {
            (Visibility::Private, why.as_deref(), share_with.clone())
        }
        Judgment::Undecided => (Visibility::Private, None, Vec::new()),
    };
    let mut shared_with = Vec::new();
    for person_id in share_with {
        if is_shareable(connection, current, person_id).await?
            && meeting_share::add(connection, current.id, person_id, SYSTEM_USER_ID).await?
        {
            shared_with.push(person_id);
        }
    }
    let changed = meeting::set_visibility(connection, current.id, after).await?;
    record(
        connection,
        current.id,
        EventKind::VisibilityChanged,
        json!({
            "before": Visibility::Unchecked.as_str(),
            "after": after.as_str(),
            "correction": true,
            "undecided": *judgment == Judgment::Undecided,
            "why": why,
            "sharedWith": shared_with,
        }),
        author,
        None,
    )
    .await?;
    if after == Visibility::Public {
        return Ok(Some(Decided {
            meeting: changed,
            raised: None,
        }));
    }
    let owner = user::find(connection, current.created_by).await?;
    let item = kept_private_item(&owner.name, *judgment == Judgment::Undecided);
    let raised = action_item::create(connection, item, Actor::System, Utc::now())
        .await
        .map_err(|error| anyhow::anyhow!("cannot raise the inbox item about who sees it: {error}"))?;
    let changed = meeting::set_visibility_item(connection, current.id, raised.record.id).await?;
    Ok(Some(Decided {
        meeting: changed,
        raised: Some(raised),
    }))
}

/// The inbox item telling a meeting's owner, `owner`, that its correction kept it private. Items
/// are the whole workspace's, so it names neither the meeting, nor why, nor whom it was shared
/// with, any of which can be what made it sensitive; the item's page opens the meeting, which says
/// all of it to those who may read it.
fn kept_private_item(owner: &str, undecided: bool) -> NewActionItem {
    let notes = if undecided {
        format!(
            "For {owner}. Elysia could not tell whether one of your meetings is sensitive, so only \
             you can read it. Open it to everyone or share it from its page."
        )
    } else {
        format!(
            "For {owner}. Elysia read one of your meetings as sensitive, so only you and the \
             people she shared it with can read it. Its page says why; open it to everyone from \
             there if it need not be private."
        )
    };
    NewActionItem {
        title: "Elysia kept a meeting private".to_owned(),
        notes,
        state: ActionItemState::Inbox,
        priority: ActionItemPriority::Normal,
        due_at: None,
        owner: Owner::User,
        project_ids: Vec::new(),
        initiative_ids: Vec::new(),
    }
}

/// Puts `current`'s visibility back to what `undone`, a person's change, changed it from, while it
/// still holds what `undone` set. `None` when it changed since.
///
/// # Errors
/// [`UndoError::NotUndoable`] for the correction's decision, which nothing would make again; any
/// database error.
pub(crate) async fn undo_visibility(
    connection: &mut AsyncPgConnection,
    current: &Meeting,
    undone: &MeetingEvent,
    author: Author<'_>,
) -> Result<Option<MeetingEvent>, UndoError> {
    if undone.data.get("correction").and_then(Value::as_bool) == Some(true) {
        return Err(UndoError::NotUndoable);
    }
    let read = |key: &str| {
        undone
            .data
            .get(key)
            .and_then(Value::as_str)
            .and_then(Visibility::parse)
    };
    let (Some(before), Some(after)) = (read("before"), read("after")) else {
        // Only an entry written by hand says neither.
        return Ok(None);
    };
    if current.visibility() != after {
        return Ok(None);
    }
    meeting::set_visibility(connection, current.id, before).await?;
    Ok(Some(
        record(
            connection,
            current.id,
            EventKind::VisibilityChanged,
            json!({ "before": after.as_str(), "after": before.as_str() }),
            author,
            Some(undone.id),
        )
        .await?,
    ))
}

/// Takes back `undone`, a share or an unshare: stops sharing the meeting with its person, or shares
/// it with them again, while it is still as `undone` left it. `None` when it changed since.
///
/// # Errors
/// Propagates any database error.
pub(crate) async fn undo_share(
    connection: &mut AsyncPgConnection,
    current: &Meeting,
    undone: &MeetingEvent,
    author: Author<'_>,
) -> Result<Option<MeetingEvent>, UndoError> {
    let Some(person_id) = undone
        .data
        .get("personId")
        .and_then(Value::as_str)
        .and_then(|person_id| person_id.parse::<Uuid>().ok())
    else {
        return Ok(None);
    };
    let (undid_by, kind) = if undone.kind() == Some(EventKind::Shared) {
        (
            meeting_share::remove(connection, current.id, person_id).await?,
            EventKind::Unshared,
        )
    } else {
        (
            meeting_share::add(connection, current.id, person_id, author.user_id).await?,
            EventKind::Shared,
        )
    };
    if !undid_by {
        return Ok(None);
    }
    Ok(Some(
        record(
            connection,
            current.id,
            kind,
            json!({ "personId": person_id }),
            author,
            Some(undone.id),
        )
        .await?,
    ))
}

/// Records a change to who sees meeting `meeting_id`.
async fn record(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    kind: EventKind,
    data: Value,
    author: Author<'_>,
    undid: Option<Uuid>,
) -> diesel::QueryResult<MeetingEvent> {
    meeting_event::record(
        connection,
        Change {
            id: Uuid::now_v7(),
            meeting_id,
            kind,
            author,
            data,
            undid,
        },
    )
    .await
}
