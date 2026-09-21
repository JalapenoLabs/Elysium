// Copyright © 2026 Jalapeno Labs

//! The history of action items and initiatives: one entry per change, naming its actor.
//!
//! Every write to an item, an initiative, a membership, or a comment records an entry in
//! the same transaction as the change, so history and state never disagree. An entry's
//! subject is an item, an initiative, or both, for an item joining or leaving one; it then
//! shows in both histories.
//!
//! `data` holds what changed, shaped by [`HistoryKind`], with before and after values
//! wherever a value was replaced, so a later change can be shown and undone from its entry.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::action_items::Actor;
use crate::database::schema::action_item_events;

/// What an entry records, and so how its `data` is shaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryKind {
    /// The record was created. `data` is its fields as created.
    Created,
    /// Fields were edited. `data.changes` maps each field to `{ from, to }`.
    Updated,
    /// An item changed state. `data` is `{ from, to }`.
    StateChanged,
    Deleted,
    Restored,
    /// `data` is `{ commentId, body }`.
    Commented,
    /// `data` is `{ commentId, from, to }`.
    CommentEdited,
    /// `data` is `{ commentId, body }`, so the comment can be put back.
    CommentDeleted,
    /// `data` is `{ projectId }`.
    ProjectAdded,
    ProjectRemoved,
    /// An item joined an initiative; the entry's subject is both. `data` is empty, or
    /// `{ linkId }` naming the container that brought it in.
    InitiativeJoined,
    InitiativeLeft,
    /// An item was linked to an external thing. `data` is the link: `{ linkId, provider,
    /// kind, key, url }`.
    LinkAdded,
    /// `data` is the link as it was, shaped like [`HistoryKind::LinkAdded`]'s.
    LinkRemoved,
    /// Another link became the item's primary. `data` is `{ from, to }`, each a link id or
    /// null.
    PrimaryLinkChanged,
    /// The user cancelled a provider write that had not landed. `data` is `{ linkId, key,
    /// write }`, where `write` is `close` or `comment`.
    LinkWriteCancelled,
    /// A linked pull request closed without merging, which resolves nothing. `data` is
    /// `{ linkId, key, url }`.
    PullRequestClosed,
    /// An initiative was linked to a container. `data` is `{ linkId, provider, kind, key,
    /// url }`.
    ContainerLinked,
    /// `data` is the container as it was, shaped like [`HistoryKind::ContainerLinked`]'s.
    ContainerUnlinked,
}

impl HistoryKind {
    /// The name stored in `kind` and sent to clients.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
            Self::StateChanged => "state_changed",
            Self::Deleted => "deleted",
            Self::Restored => "restored",
            Self::Commented => "commented",
            Self::CommentEdited => "comment_edited",
            Self::CommentDeleted => "comment_deleted",
            Self::ProjectAdded => "project_added",
            Self::ProjectRemoved => "project_removed",
            Self::InitiativeJoined => "initiative_joined",
            Self::InitiativeLeft => "initiative_left",
            Self::LinkAdded => "link_added",
            Self::LinkRemoved => "link_removed",
            Self::PrimaryLinkChanged => "primary_link_changed",
            Self::LinkWriteCancelled => "link_write_cancelled",
            Self::PullRequestClosed => "pull_request_closed",
            Self::ContainerLinked => "container_linked",
            Self::ContainerUnlinked => "container_unlinked",
        }
    }
}

/// A stored history entry.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = action_item_events, check_for_backend(diesel::pg::Pg))]
pub struct HistoryEntry {
    pub id: Uuid,
    pub action_item_id: Option<Uuid>,
    pub initiative_id: Option<Uuid>,
    /// A [`HistoryKind`] name. Kept as text so entries written by later versions still load.
    pub kind: String,
    /// An [`Actor`] in its recorded form.
    pub actor: String,
    pub data: Value,
    pub created_at: DateTime<Utc>,
}

/// What an entry is about. An item joining or leaving an initiative is about both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    Item(Uuid),
    Initiative(Uuid),
    Membership { item: Uuid, initiative: Uuid },
}

#[derive(Insertable)]
#[diesel(table_name = action_item_events)]
struct HistoryRow {
    id: Uuid,
    action_item_id: Option<Uuid>,
    initiative_id: Option<Uuid>,
    kind: &'static str,
    actor: String,
    data: Value,
    created_at: DateTime<Utc>,
}

/// One change to record: what it was about, what kind it was, who made it, and when.
#[derive(Debug)]
pub struct Change {
    pub subject: Subject,
    pub kind: HistoryKind,
    pub actor: Actor,
    pub data: Value,
    pub at: DateTime<Utc>,
}

/// `{ from, to }`, the shape every replaced value takes in history.
pub fn replaced(from: impl Serialize, to: impl Serialize) -> Value {
    json!({ "from": from, "to": to })
}

/// A record as a write left it, with the history entries the write recorded.
#[derive(Debug)]
pub struct Recorded<Record> {
    pub record: Record,
    /// Oldest first; empty when the write turned out to change nothing.
    pub history: Vec<HistoryEntry>,
}

/// Records one change. Call it inside the transaction that makes the change.
///
/// # Errors
/// Propagates any database error.
pub async fn record(
    connection: &mut AsyncPgConnection,
    change: Change,
) -> QueryResult<HistoryEntry> {
    let (action_item_id, initiative_id) = match change.subject {
        Subject::Item(item) => (Some(item), None),
        Subject::Initiative(initiative) => (None, Some(initiative)),
        Subject::Membership { item, initiative } => (Some(item), Some(initiative)),
    };
    diesel::insert_into(action_item_events::table)
        .values(HistoryRow {
            id: Uuid::now_v7(),
            action_item_id,
            initiative_id,
            kind: change.kind.as_str(),
            actor: change.actor.to_string(),
            data: change.data,
            created_at: change.at,
        })
        .returning(HistoryEntry::as_returning())
        .get_result(connection)
        .await
}

/// An item's history, oldest first, including its joining and leaving initiatives.
///
/// # Errors
/// Propagates any database error.
pub async fn list_for_item(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
) -> QueryResult<Vec<HistoryEntry>> {
    action_item_events::table
        .filter(action_item_events::action_item_id.eq(action_item_id))
        .order(action_item_events::id.asc())
        .select(HistoryEntry::as_select())
        .load(connection)
        .await
}

/// An initiative's history, oldest first, including items joining and leaving it.
///
/// # Errors
/// Propagates any database error.
pub async fn list_for_initiative(
    connection: &mut AsyncPgConnection,
    initiative_id: Uuid,
) -> QueryResult<Vec<HistoryEntry>> {
    action_item_events::table
        .filter(action_item_events::initiative_id.eq(initiative_id))
        .order(action_item_events::id.asc())
        .select(HistoryEntry::as_select())
        .load(connection)
        .await
}
