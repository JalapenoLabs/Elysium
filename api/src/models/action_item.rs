// Copyright © 2026 Jalapeno Labs

//! Action items and their memberships in projects and initiatives.
//!
//! Every write takes the [`Actor`] making it and the moment it happens, and records its
//! history in the same transaction (see `crate::models::action_item_event`). Writes lock
//! the item's row first, so two changes to one item apply one after the other and each
//! sees the state the other left.
//!
//! Deleting is soft: `deleted_at` hides an item from every list and from Next, and every
//! write but [`restore`] refuses a deleted item.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::action_items::{Actor, Transition, WorkError, next};
use crate::database::schema::{action_item_projects, action_items, initiative_items, initiatives};
use crate::models::action_item_event::{
    self, Change, HistoryEntry, HistoryKind, Recorded, Subject,
};
use crate::models::project;

/// Where an item stands. See `docs/action-items.md`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::ActionItemState"]
#[serde(rename_all = "kebab-case")]
pub enum ActionItemState {
    /// Arrived but not accepted.
    Inbox,
    /// Accepted; the user intends to do it.
    Open,
    /// Done: the item was acted on.
    Resolved,
    /// Will not be done.
    Dismissed,
}

/// How urgent an item is. Declared most urgent first, so the derived order is Next's.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    diesel_derive_enum::DbEnum,
    Serialize,
    Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::ActionItemPriority"]
#[serde(rename_all = "kebab-case")]
pub enum ActionItemPriority {
    Urgent,
    High,
    Normal,
    Low,
}

/// The `owner_kind` column; [`Owner`] is the same with the other person's name.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::ActionItemOwnerKind"]
#[serde(rename_all = "kebab-case")]
pub enum OwnerKind {
    User,
    Other,
    Nobody,
}

/// Whose list an item is on. Only the user's items appear in Next.
///
/// There is no users table yet, so someone else is recorded by the name or address a
/// provider reports, and `Nobody` is a linked issue with no assignee.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Owner {
    User,
    Other { name: String },
    Nobody,
}

impl Owner {
    /// The `owner_kind` and `owner_name` columns for this owner.
    fn into_columns(self) -> (OwnerKind, Option<String>) {
        match self {
            Self::User => (OwnerKind::User, None),
            Self::Other { name } => (OwnerKind::Other, Some(name)),
            Self::Nobody => (OwnerKind::Nobody, None),
        }
    }
}

/// A stored action item.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = action_items, check_for_backend(diesel::pg::Pg))]
pub struct ActionItem {
    pub id: Uuid,
    pub title: String,
    pub notes: String,
    pub state: ActionItemState,
    pub priority: ActionItemPriority,
    pub due_at: Option<DateTime<Utc>>,
    /// Hidden from Next until this moment passes.
    pub snoozed_until: Option<DateTime<Utc>>,
    /// Someone else owes the next step.
    pub waiting_on: Option<String>,
    pub owner_kind: OwnerKind,
    /// Set exactly when `owner_kind` is `Other`.
    pub owner_name: Option<String>,
    /// Set exactly while the item is resolved.
    pub resolved_at: Option<DateTime<Utc>>,
    /// Set exactly while the item is dismissed.
    pub dismissed_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ActionItem {
    /// Whose list the item is on.
    ///
    /// # Panics
    /// Panics if an `Other` owner has no name, which `action_items_owner_name_set` rules out.
    pub fn owner(&self) -> Owner {
        match self.owner_kind {
            OwnerKind::User => Owner::User,
            OwnerKind::Other => Owner::Other {
                name: self
                    .owner_name
                    .clone()
                    .expect("action_items_owner_name_set requires a name"),
            },
            OwnerKind::Nobody => Owner::Nobody,
        }
    }
}

/// Fields for a new item.
#[derive(Debug)]
pub struct NewActionItem {
    pub title: String,
    pub notes: String,
    /// `Inbox` for an item nobody accepted yet, `Open` for one the user created or accepted.
    pub state: ActionItemState,
    pub priority: ActionItemPriority,
    pub due_at: Option<DateTime<Utc>>,
    pub owner: Owner,
    pub project_ids: Vec<Uuid>,
    pub initiative_ids: Vec<Uuid>,
}

/// A partial update. `None` leaves a field untouched; for the nullable fields, `Some(None)`
/// clears it.
#[derive(Debug, Default)]
#[expect(
    clippy::option_option,
    reason = "absent, cleared, and a value are three distinct requests"
)]
pub struct ActionItemChanges {
    pub title: Option<String>,
    pub notes: Option<String>,
    pub priority: Option<ActionItemPriority>,
    pub due_at: Option<Option<DateTime<Utc>>>,
    pub snoozed_until: Option<Option<DateTime<Utc>>>,
    pub waiting_on: Option<Option<String>>,
    pub owner: Option<Owner>,
}

impl ActionItemChanges {
    /// True when there is nothing to apply.
    pub const fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.notes.is_none()
            && self.priority.is_none()
            && self.due_at.is_none()
            && self.snoozed_until.is_none()
            && self.waiting_on.is_none()
            && self.owner.is_none()
    }
}

/// Which items a list returns. The default is every item that is not deleted.
#[derive(Debug, Default)]
pub struct ActionItemFilter {
    /// Items in any of these states; empty for every state.
    pub states: Vec<ActionItemState>,
    pub project: Option<ProjectFilter>,
    /// Items currently in this initiative.
    pub initiative: Option<Uuid>,
    /// Items waiting on someone, or not.
    pub waiting: Option<bool>,
    /// Items snoozed past the moment of the query, or not.
    pub snoozed: Option<bool>,
    /// Deleted items only, instead of the rest.
    pub deleted: bool,
}

/// Items in one project, or in none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectFilter {
    Unassigned,
    Project(Uuid),
}

/// The projects and initiatives a set of items belong to.
#[derive(Debug, Default)]
pub struct Memberships {
    projects: HashMap<Uuid, Vec<Uuid>>,
    initiatives: HashMap<Uuid, Vec<Uuid>>,
}

impl Memberships {
    pub fn project_ids(&self, action_item_id: Uuid) -> Vec<Uuid> {
        self.projects
            .get(&action_item_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn initiative_ids(&self, action_item_id: Uuid) -> Vec<Uuid> {
        self.initiatives
            .get(&action_item_id)
            .cloned()
            .unwrap_or_default()
    }
}

#[derive(Insertable)]
#[diesel(table_name = action_items)]
struct ActionItemRow {
    id: Uuid,
    title: String,
    notes: String,
    state: ActionItemState,
    priority: ActionItemPriority,
    due_at: Option<DateTime<Utc>>,
    owner_kind: OwnerKind,
    owner_name: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(Default, AsChangeset)]
#[diesel(table_name = action_items)]
#[expect(
    clippy::option_option,
    reason = "Diesel's changeset encoding: outer None skips, Some(None) writes NULL"
)]
struct ActionItemChangeset {
    title: Option<String>,
    notes: Option<String>,
    priority: Option<ActionItemPriority>,
    due_at: Option<Option<DateTime<Utc>>>,
    snoozed_until: Option<Option<DateTime<Utc>>>,
    waiting_on: Option<Option<String>>,
    owner_kind: Option<OwnerKind>,
    owner_name: Option<Option<String>>,
}

#[derive(Insertable)]
#[diesel(table_name = action_item_projects)]
struct ProjectLinkRow {
    action_item_id: Uuid,
    project_id: Uuid,
}

#[derive(Insertable)]
#[diesel(table_name = initiative_items)]
struct InitiativeSpanRow {
    id: Uuid,
    initiative_id: Uuid,
    action_item_id: Uuid,
    joined_at: DateTime<Utc>,
}

/// `{ from, to }`, the shape every replaced value takes in history.
fn replaced(from: impl Serialize, to: impl Serialize) -> Value {
    json!({ "from": from, "to": to })
}

/// Loads an item for a write, locking its row until the transaction ends.
async fn lock(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<ActionItem> {
    action_items::table
        .find(id)
        .for_update()
        .select(ActionItem::as_select())
        .first(connection)
        .await
}

/// Loads a live item for a write, locking its row until the transaction ends.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id and
/// [`WorkError::Conflict`] for a deleted item.
pub async fn lock_live(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> Result<ActionItem, WorkError> {
    let item = lock(connection, id).await?;
    if item.deleted_at.is_some() {
        return Err(WorkError::Conflict("the item is deleted; restore it first"));
    }
    Ok(item)
}

/// One item by id, deleted or not.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<ActionItem> {
    action_items::table
        .find(id)
        .select(ActionItem::as_select())
        .first(connection)
        .await
}

/// The items `filter` selects, newest first. `now` decides which items are snoozed.
///
/// # Errors
/// Propagates any database error.
pub async fn list(
    connection: &mut AsyncPgConnection,
    filter: &ActionItemFilter,
    now: DateTime<Utc>,
) -> QueryResult<Vec<ActionItem>> {
    let mut query = action_items::table.into_boxed();

    query = if filter.deleted {
        query.filter(action_items::deleted_at.is_not_null())
    } else {
        query.filter(action_items::deleted_at.is_null())
    };
    if !filter.states.is_empty() {
        query = query.filter(action_items::state.eq_any(filter.states.clone()));
    }
    match filter.project {
        None => {}
        Some(ProjectFilter::Project(project_id)) => {
            let linked = action_item_projects::table
                .filter(action_item_projects::project_id.eq(project_id))
                .select(action_item_projects::action_item_id);
            query = query.filter(action_items::id.eq_any(linked));
        }
        Some(ProjectFilter::Unassigned) => {
            let linked = action_item_projects::table.select(action_item_projects::action_item_id);
            query = query.filter(diesel::dsl::not(action_items::id.eq_any(linked)));
        }
    }
    if let Some(initiative_id) = filter.initiative {
        let members = initiative_items::table
            .filter(initiative_items::initiative_id.eq(initiative_id))
            .filter(initiative_items::left_at.is_null())
            .select(initiative_items::action_item_id);
        query = query.filter(action_items::id.eq_any(members));
    }
    match filter.waiting {
        None => {}
        Some(true) => query = query.filter(action_items::waiting_on.is_not_null()),
        Some(false) => query = query.filter(action_items::waiting_on.is_null()),
    }
    match filter.snoozed {
        None => {}
        Some(true) => query = query.filter(action_items::snoozed_until.gt(now)),
        Some(false) => {
            query = query.filter(
                action_items::snoozed_until
                    .is_null()
                    .or(action_items::snoozed_until.le(now)),
            );
        }
    }

    query
        .order((action_items::created_at.desc(), action_items::id.desc()))
        .select(ActionItem::as_select())
        .load(connection)
        .await
}

/// Next as of `now`, in order, and how many items wait in the inbox.
///
/// An item is in Next when it is open, the user's, not deleted, not waiting on anyone,
/// and not snoozed past `now`. The order is [`next::order`].
///
/// # Errors
/// Propagates any database error.
pub async fn next(
    connection: &mut AsyncPgConnection,
    now: DateTime<Utc>,
) -> QueryResult<(Vec<ActionItem>, i64)> {
    let mut items = action_items::table
        .filter(action_items::deleted_at.is_null())
        .filter(action_items::state.eq(ActionItemState::Open))
        .filter(action_items::owner_kind.eq(OwnerKind::User))
        .filter(action_items::waiting_on.is_null())
        .filter(
            action_items::snoozed_until
                .is_null()
                .or(action_items::snoozed_until.le(now)),
        )
        .select(ActionItem::as_select())
        .load(connection)
        .await?;
    next::order(&mut items, now);

    let inbox_count = action_items::table
        .filter(action_items::deleted_at.is_null())
        .filter(action_items::state.eq(ActionItemState::Inbox))
        .count()
        .get_result(connection)
        .await?;
    Ok((items, inbox_count))
}

/// The projects each of `action_item_ids` belongs to, and the initiatives it is in now.
/// Deleted initiatives are left out.
///
/// # Errors
/// Propagates any database error.
pub async fn memberships(
    connection: &mut AsyncPgConnection,
    action_item_ids: &[Uuid],
) -> QueryResult<Memberships> {
    let project_links: Vec<(Uuid, Uuid)> = action_item_projects::table
        .filter(action_item_projects::action_item_id.eq_any(action_item_ids))
        .order((
            action_item_projects::action_item_id,
            action_item_projects::project_id,
        ))
        .select((
            action_item_projects::action_item_id,
            action_item_projects::project_id,
        ))
        .load(connection)
        .await?;
    let initiative_links: Vec<(Uuid, Uuid)> = initiative_items::table
        .inner_join(initiatives::table)
        .filter(initiative_items::action_item_id.eq_any(action_item_ids))
        .filter(initiative_items::left_at.is_null())
        .filter(initiatives::deleted_at.is_null())
        .order((initiative_items::action_item_id, initiatives::id))
        .select((initiative_items::action_item_id, initiatives::id))
        .load(connection)
        .await?;

    let mut memberships = Memberships::default();
    for (action_item_id, project_id) in project_links {
        memberships
            .projects
            .entry(action_item_id)
            .or_default()
            .push(project_id);
    }
    for (action_item_id, initiative_id) in initiative_links {
        memberships
            .initiatives
            .entry(action_item_id)
            .or_default()
            .push(initiative_id);
    }
    Ok(memberships)
}

/// Every initiative the item is in now, deleted or not. A change to the item changes
/// their progress.
///
/// # Errors
/// Propagates any database error.
pub async fn current_initiative_ids(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
) -> QueryResult<Vec<Uuid>> {
    initiative_items::table
        .filter(initiative_items::action_item_id.eq(action_item_id))
        .filter(initiative_items::left_at.is_null())
        .select(initiative_items::initiative_id)
        .load(connection)
        .await
}

/// Refuses initiative ids that name no initiative, or a deleted one.
async fn require_live_initiatives(
    connection: &mut AsyncPgConnection,
    initiative_ids: &[Uuid],
) -> Result<(), WorkError> {
    let live: i64 = initiatives::table
        .filter(initiatives::id.eq_any(initiative_ids))
        .filter(initiatives::deleted_at.is_null())
        .count()
        .get_result(connection)
        .await?;
    if usize::try_from(live).ok() != Some(initiative_ids.len()) {
        return Err(WorkError::Invalid(
            "initiativeIds lists an initiative that does not exist or is deleted",
        ));
    }
    Ok(())
}

/// Opens a span of the item in the initiative, and records it on both.
async fn join(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
    initiative_id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> QueryResult<HistoryEntry> {
    diesel::insert_into(initiative_items::table)
        .values(InitiativeSpanRow {
            id: Uuid::now_v7(),
            initiative_id,
            action_item_id,
            joined_at: now,
        })
        .execute(connection)
        .await?;
    action_item_event::record(
        connection,
        Change {
            subject: Subject::Membership {
                item: action_item_id,
                initiative: initiative_id,
            },
            kind: HistoryKind::InitiativeJoined,
            actor,
            data: json!({}),
            at: now,
        },
    )
    .await
}

/// Creates an item with a new `UUIDv7` id, in its projects and initiatives.
///
/// Project and initiative ids must be sorted and unique.
///
/// # Errors
/// Returns [`WorkError::Invalid`] for an initiative that does not exist or is deleted,
/// and propagates database errors, including a foreign key violation for a project that
/// does not exist.
pub async fn create(
    connection: &mut AsyncPgConnection,
    new_item: NewActionItem,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    let id = Uuid::now_v7();
    let created = json!({
        "title": new_item.title,
        "notes": new_item.notes,
        "state": new_item.state,
        "priority": new_item.priority,
        "dueAt": new_item.due_at,
        "owner": new_item.owner,
        "projectIds": new_item.project_ids,
    });
    let (owner_kind, owner_name) = new_item.owner.into_columns();
    let row = ActionItemRow {
        id,
        title: new_item.title,
        notes: new_item.notes,
        state: new_item.state,
        priority: new_item.priority,
        due_at: new_item.due_at,
        owner_kind,
        owner_name,
        created_at: now,
    };
    let project_links: Vec<ProjectLinkRow> = new_item
        .project_ids
        .iter()
        .map(|&project_id| ProjectLinkRow {
            action_item_id: id,
            project_id,
        })
        .collect();
    let initiative_ids = new_item.initiative_ids;

    connection
        .transaction(async move |connection| {
            require_live_initiatives(connection, &initiative_ids).await?;
            diesel::insert_into(action_items::table)
                .values(row)
                .execute(connection)
                .await?;
            diesel::insert_into(action_item_projects::table)
                .values(project_links)
                .execute(connection)
                .await?;

            let mut history = vec![
                action_item_event::record(
                    connection,
                    Change {
                        subject: Subject::Item(id),
                        kind: HistoryKind::Created,
                        actor,
                        data: created,
                        at: now,
                    },
                )
                .await?,
            ];
            for initiative_id in initiative_ids {
                history.push(join(connection, id, initiative_id, actor, now).await?);
            }

            let record = find(connection, id).await?;
            Ok(Recorded { record, history })
        })
        .await
}

/// Applies `changes` to a live item, recording the fields that actually changed. Values
/// equal to the stored ones change nothing and record nothing.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id, [`WorkError::Conflict`]
/// for a deleted item, and any other database error.
pub async fn update(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    changes: ActionItemChanges,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    connection
        .transaction(async move |connection| {
            let item = lock_live(connection, id).await?;
            let mut diff = Map::new();
            let mut changeset = ActionItemChangeset::default();

            if let Some(title) = changes.title
                && title != item.title
            {
                diff.insert("title".to_owned(), replaced(&item.title, &title));
                changeset.title = Some(title);
            }
            if let Some(notes) = changes.notes
                && notes != item.notes
            {
                diff.insert("notes".to_owned(), replaced(&item.notes, &notes));
                changeset.notes = Some(notes);
            }
            if let Some(priority) = changes.priority
                && priority != item.priority
            {
                diff.insert("priority".to_owned(), replaced(item.priority, priority));
                changeset.priority = Some(priority);
            }
            if let Some(due_at) = changes.due_at
                && due_at != item.due_at
            {
                diff.insert("dueAt".to_owned(), replaced(item.due_at, due_at));
                changeset.due_at = Some(due_at);
            }
            if let Some(snoozed_until) = changes.snoozed_until
                && snoozed_until != item.snoozed_until
            {
                diff.insert(
                    "snoozedUntil".to_owned(),
                    replaced(item.snoozed_until, snoozed_until),
                );
                changeset.snoozed_until = Some(snoozed_until);
            }
            if let Some(waiting_on) = changes.waiting_on
                && waiting_on != item.waiting_on
            {
                diff.insert(
                    "waitingOn".to_owned(),
                    replaced(&item.waiting_on, &waiting_on),
                );
                changeset.waiting_on = Some(waiting_on);
            }
            if let Some(owner) = changes.owner
                && owner != item.owner()
            {
                diff.insert("owner".to_owned(), replaced(item.owner(), &owner));
                let (owner_kind, owner_name) = owner.into_columns();
                changeset.owner_kind = Some(owner_kind);
                changeset.owner_name = Some(owner_name);
            }

            if diff.is_empty() {
                return Ok(Recorded {
                    record: item,
                    history: Vec::new(),
                });
            }

            let record = diesel::update(action_items::table.find(id))
                .set(changeset)
                .returning(ActionItem::as_returning())
                .get_result(connection)
                .await?;
            let entry = action_item_event::record(
                connection,
                Change {
                    subject: Subject::Item(id),
                    kind: HistoryKind::Updated,
                    actor,
                    data: json!({ "changes": diff }),
                    at: now,
                },
            )
            .await?;
            Ok(Recorded {
                record,
                history: vec![entry],
            })
        })
        .await
}

/// Moves a live item to another state, stamping when it was resolved or dismissed.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id, [`WorkError::Conflict`]
/// for a deleted item or a transition its state does not allow, and any other database
/// error.
pub async fn transition(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    transition: Transition,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    connection
        .transaction(async move |connection| {
            let item = lock_live(connection, id).await?;
            let state = transition.apply(item.state).map_err(WorkError::Conflict)?;
            let resolved_at = (state == ActionItemState::Resolved).then_some(now);
            let dismissed_at = (state == ActionItemState::Dismissed).then_some(now);

            let record = diesel::update(action_items::table.find(id))
                .set((
                    action_items::state.eq(state),
                    action_items::resolved_at.eq(resolved_at),
                    action_items::dismissed_at.eq(dismissed_at),
                ))
                .returning(ActionItem::as_returning())
                .get_result(connection)
                .await?;
            let entry = action_item_event::record(
                connection,
                Change {
                    subject: Subject::Item(id),
                    kind: HistoryKind::StateChanged,
                    actor,
                    data: replaced(item.state, state),
                    at: now,
                },
            )
            .await?;
            Ok(Recorded {
                record,
                history: vec![entry],
            })
        })
        .await
}

/// Deletes an item softly: it is hidden everywhere until restored.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id, [`WorkError::Conflict`]
/// when it is already deleted, and any other database error.
pub async fn soft_delete(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    connection
        .transaction(async move |connection| {
            lock_live(connection, id).await?;
            let record = diesel::update(action_items::table.find(id))
                .set(action_items::deleted_at.eq(now))
                .returning(ActionItem::as_returning())
                .get_result(connection)
                .await?;
            let entry = action_item_event::record(
                connection,
                Change {
                    subject: Subject::Item(id),
                    kind: HistoryKind::Deleted,
                    actor,
                    data: json!({}),
                    at: now,
                },
            )
            .await?;
            Ok(Recorded {
                record,
                history: vec![entry],
            })
        })
        .await
}

/// Brings a deleted item back as it was.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id, [`WorkError::Conflict`]
/// when it is not deleted, and any other database error.
pub async fn restore(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    connection
        .transaction(async move |connection| {
            let item = lock(connection, id).await?;
            let Some(deleted_at) = item.deleted_at else {
                return Err(WorkError::Conflict("the item is not deleted"));
            };
            let record = diesel::update(action_items::table.find(id))
                .set(action_items::deleted_at.eq(None::<DateTime<Utc>>))
                .returning(ActionItem::as_returning())
                .get_result(connection)
                .await?;
            let entry = action_item_event::record(
                connection,
                Change {
                    subject: Subject::Item(id),
                    kind: HistoryKind::Restored,
                    actor,
                    data: json!({ "deletedAt": deleted_at }),
                    at: now,
                },
            )
            .await?;
            Ok(Recorded {
                record,
                history: vec![entry],
            })
        })
        .await
}

/// Adds a live item to a project. Adding it again changes nothing.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown item or project,
/// [`WorkError::Conflict`] for a deleted item, and any other database error.
pub async fn add_project(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    project_id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    connection
        .transaction(async move |connection| {
            let item = lock_live(connection, id).await?;
            project::find(connection, project_id).await?;
            let inserted = diesel::insert_into(action_item_projects::table)
                .values(ProjectLinkRow {
                    action_item_id: id,
                    project_id,
                })
                .on_conflict_do_nothing()
                .execute(connection)
                .await?;
            if inserted == 0 {
                return Ok(Recorded {
                    record: item,
                    history: Vec::new(),
                });
            }
            let entry = action_item_event::record(
                connection,
                Change {
                    subject: Subject::Item(id),
                    kind: HistoryKind::ProjectAdded,
                    actor,
                    data: json!({ "projectId": project_id }),
                    at: now,
                },
            )
            .await?;
            Ok(Recorded {
                record: item,
                history: vec![entry],
            })
        })
        .await
}

/// Takes a live item out of a project. Removing it when it is not there changes nothing.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown item, [`WorkError::Conflict`]
/// for a deleted item, and any other database error.
pub async fn remove_project(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    project_id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    connection
        .transaction(async move |connection| {
            let item = lock_live(connection, id).await?;
            let removed = diesel::delete(
                action_item_projects::table
                    .filter(action_item_projects::action_item_id.eq(id))
                    .filter(action_item_projects::project_id.eq(project_id)),
            )
            .execute(connection)
            .await?;
            if removed == 0 {
                return Ok(Recorded {
                    record: item,
                    history: Vec::new(),
                });
            }
            let entry = action_item_event::record(
                connection,
                Change {
                    subject: Subject::Item(id),
                    kind: HistoryKind::ProjectRemoved,
                    actor,
                    data: json!({ "projectId": project_id }),
                    at: now,
                },
            )
            .await?;
            Ok(Recorded {
                record: item,
                history: vec![entry],
            })
        })
        .await
}

/// Puts a live item into a live initiative. Joining one it is already in changes nothing.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown item or initiative,
/// [`WorkError::Conflict`] when either is deleted, and any other database error.
pub async fn join_initiative(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    initiative_id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    connection
        .transaction(async move |connection| {
            let item = lock_live(connection, id).await?;
            let initiative_deleted_at: Option<DateTime<Utc>> = initiatives::table
                .find(initiative_id)
                .select(initiatives::deleted_at)
                .first(connection)
                .await?;
            if initiative_deleted_at.is_some() {
                return Err(WorkError::Conflict("the initiative is deleted"));
            }
            if current_initiative_ids(connection, id)
                .await?
                .contains(&initiative_id)
            {
                return Ok(Recorded {
                    record: item,
                    history: Vec::new(),
                });
            }
            let entry = join(connection, id, initiative_id, actor, now).await?;
            Ok(Recorded {
                record: item,
                history: vec![entry],
            })
        })
        .await
}

/// Takes a live item out of an initiative, closing its span so progress over time keeps
/// it. Leaving one it is not in changes nothing.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown item, [`WorkError::Conflict`]
/// for a deleted item, and any other database error.
pub async fn leave_initiative(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    initiative_id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    connection
        .transaction(async move |connection| {
            let item = lock_live(connection, id).await?;
            let closed = diesel::update(
                initiative_items::table
                    .filter(initiative_items::action_item_id.eq(id))
                    .filter(initiative_items::initiative_id.eq(initiative_id))
                    .filter(initiative_items::left_at.is_null()),
            )
            .set(initiative_items::left_at.eq(now))
            .execute(connection)
            .await?;
            if closed == 0 {
                return Ok(Recorded {
                    record: item,
                    history: Vec::new(),
                });
            }
            let entry = action_item_event::record(
                connection,
                Change {
                    subject: Subject::Membership {
                        item: id,
                        initiative: initiative_id,
                    },
                    kind: HistoryKind::InitiativeLeft,
                    actor,
                    data: json!({}),
                    at: now,
                },
            )
            .await?;
            Ok(Recorded {
                record: item,
                history: vec![entry],
            })
        })
        .await
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::models::action_item_event::list_for_item;
    use crate::models::initiative::{self, NewInitiative};
    use crate::models::project::NewProject;
    use crate::test_support::migrated_database;

    /// A fixed moment the tests count from, so ages and due dates are exact.
    fn minute(offset: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z")
            .expect("a valid instant")
            .with_timezone(&Utc)
            + TimeDelta::minutes(offset)
    }

    fn new_item(title: &str) -> NewActionItem {
        NewActionItem {
            title: title.to_owned(),
            notes: String::new(),
            state: ActionItemState::Open,
            priority: ActionItemPriority::Normal,
            due_at: None,
            owner: Owner::User,
            project_ids: Vec::new(),
            initiative_ids: Vec::new(),
        }
    }

    fn new_initiative(name: &str) -> NewInitiative {
        NewInitiative {
            name: name.to_owned(),
            description: String::new(),
            target_at: None,
            project_ids: Vec::new(),
        }
    }

    async fn create_at(
        connection: &mut AsyncPgConnection,
        item: NewActionItem,
        at: DateTime<Utc>,
    ) -> ActionItem {
        create(connection, item, Actor::User, at)
            .await
            .expect("create")
            .record
    }

    async fn change_at(
        connection: &mut AsyncPgConnection,
        id: Uuid,
        changes: ActionItemChanges,
        at: DateTime<Utc>,
    ) {
        update(connection, id, changes, Actor::User, at)
            .await
            .expect("update");
    }

    async fn history_kinds(connection: &mut AsyncPgConnection, id: Uuid) -> Vec<String> {
        list_for_item(connection, id)
            .await
            .expect("history")
            .into_iter()
            .map(|entry| entry.kind)
            .collect()
    }

    fn titles(items: &[ActionItem]) -> Vec<&str> {
        items.iter().map(|item| item.title.as_str()).collect()
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn next_holds_the_users_open_items_that_wait_on_nothing_in_order() {
        let (_url, mut connection) = migrated_database().await;
        let now = minute(1_000);

        let urgent = NewActionItem {
            priority: ActionItemPriority::Urgent,
            ..new_item("urgent")
        };
        create_at(&mut connection, urgent, minute(0)).await;
        let overdue = NewActionItem {
            priority: ActionItemPriority::Low,
            due_at: Some(now - TimeDelta::hours(1)),
            ..new_item("overdue")
        };
        create_at(&mut connection, overdue, minute(1)).await;
        create_at(&mut connection, new_item("older normal"), minute(2)).await;
        create_at(&mut connection, new_item("newer normal"), minute(3)).await;

        // Everything from here to the woken item stays out of Next.
        let inbox = NewActionItem {
            state: ActionItemState::Inbox,
            ..new_item("inbox")
        };
        create_at(&mut connection, inbox, minute(4)).await;
        let theirs = NewActionItem {
            owner: Owner::Other {
                name: "sam@example.com".to_owned(),
            },
            ..new_item("theirs")
        };
        create_at(&mut connection, theirs, minute(5)).await;
        let unowned = NewActionItem {
            owner: Owner::Nobody,
            ..new_item("unowned")
        };
        create_at(&mut connection, unowned, minute(6)).await;
        let waiting = create_at(&mut connection, new_item("waiting"), minute(7)).await;
        let wait = ActionItemChanges {
            waiting_on: Some(Some("Sam".to_owned())),
            ..ActionItemChanges::default()
        };
        change_at(&mut connection, waiting.id, wait, minute(8)).await;
        let snoozed = create_at(&mut connection, new_item("snoozed"), minute(9)).await;
        let snooze = ActionItemChanges {
            snoozed_until: Some(Some(now + TimeDelta::hours(1))),
            ..ActionItemChanges::default()
        };
        change_at(&mut connection, snoozed.id, snooze, minute(10)).await;
        let resolved = create_at(&mut connection, new_item("resolved"), minute(11)).await;
        transition(
            &mut connection,
            resolved.id,
            Transition::Resolve,
            Actor::User,
            minute(12),
        )
        .await
        .expect("resolve");
        let deleted = create_at(&mut connection, new_item("deleted"), minute(13)).await;
        soft_delete(&mut connection, deleted.id, Actor::User, minute(14))
            .await
            .expect("delete");

        // A snooze that has already ended no longer hides its item.
        let woken = create_at(&mut connection, new_item("woken"), minute(15)).await;
        let past_snooze = ActionItemChanges {
            snoozed_until: Some(Some(now - TimeDelta::minutes(1))),
            ..ActionItemChanges::default()
        };
        change_at(&mut connection, woken.id, past_snooze, minute(16)).await;

        let (items, inbox_count) = next(&mut connection, now).await.expect("next");

        assert_eq!(
            titles(&items),
            ["overdue", "urgent", "older normal", "newer normal", "woken"]
        );
        assert_eq!(inbox_count, 1);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn transitions_stamp_and_clear_resolution_and_refuse_what_the_state_forbids() {
        let (_url, mut connection) = migrated_database().await;
        let inbox = NewActionItem {
            state: ActionItemState::Inbox,
            ..new_item("triage me")
        };
        let id = create_at(&mut connection, inbox, minute(0)).await.id;

        let reopen_inbox = transition(
            &mut connection,
            id,
            Transition::Reopen,
            Actor::User,
            minute(1),
        )
        .await;
        assert!(matches!(reopen_inbox, Err(WorkError::Conflict(_))));

        let accepted = transition(
            &mut connection,
            id,
            Transition::Accept,
            Actor::User,
            minute(1),
        )
        .await
        .expect("accept");
        assert_eq!(accepted.record.state, ActionItemState::Open);
        assert_eq!(
            accepted.history[0].data,
            json!({ "from": "inbox", "to": "open" })
        );
        assert_eq!(accepted.history[0].actor, "user");

        let resolved = transition(
            &mut connection,
            id,
            Transition::Resolve,
            Actor::User,
            minute(2),
        )
        .await
        .expect("resolve")
        .record;
        assert_eq!(resolved.state, ActionItemState::Resolved);
        assert_eq!(resolved.resolved_at, Some(minute(2)));

        let dismiss_resolved = transition(
            &mut connection,
            id,
            Transition::Dismiss,
            Actor::User,
            minute(3),
        )
        .await;
        assert!(matches!(dismiss_resolved, Err(WorkError::Conflict(_))));

        let reopened = transition(
            &mut connection,
            id,
            Transition::Reopen,
            Actor::User,
            minute(3),
        )
        .await
        .expect("reopen")
        .record;
        assert_eq!(reopened.state, ActionItemState::Open);
        assert_eq!(
            reopened.resolved_at, None,
            "reopening clears the resolution"
        );

        let dismissed = transition(
            &mut connection,
            id,
            Transition::Dismiss,
            Actor::User,
            minute(4),
        )
        .await
        .expect("dismiss")
        .record;
        assert_eq!(
            (dismissed.dismissed_at, dismissed.resolved_at),
            (Some(minute(4)), None)
        );

        assert_eq!(
            history_kinds(&mut connection, id).await,
            [
                "created",
                "state_changed",
                "state_changed",
                "state_changed",
                "state_changed"
            ],
            "refused transitions record nothing"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_deleted_item_is_hidden_refuses_writes_and_restores_as_it_was() {
        let (_url, mut connection) = migrated_database().await;
        let id = create_at(&mut connection, new_item("tidy up"), minute(0))
            .await
            .id;
        let everything = ActionItemFilter::default();
        let trash = ActionItemFilter {
            deleted: true,
            ..ActionItemFilter::default()
        };

        let deleted = soft_delete(&mut connection, id, Actor::User, minute(1))
            .await
            .expect("delete")
            .record;
        assert_eq!(deleted.deleted_at, Some(minute(1)));
        let listed = list(&mut connection, &everything, minute(2))
            .await
            .expect("list");
        assert!(listed.is_empty());
        let in_trash = list(&mut connection, &trash, minute(2))
            .await
            .expect("list");
        assert_eq!(titles(&in_trash), ["tidy up"]);

        let rename = ActionItemChanges {
            title: Some("renamed".to_owned()),
            ..ActionItemChanges::default()
        };
        let refused = update(&mut connection, id, rename, Actor::User, minute(2)).await;
        assert!(matches!(refused, Err(WorkError::Conflict(_))));
        let resolve = transition(
            &mut connection,
            id,
            Transition::Resolve,
            Actor::User,
            minute(2),
        )
        .await;
        assert!(matches!(resolve, Err(WorkError::Conflict(_))));
        let again = soft_delete(&mut connection, id, Actor::User, minute(2)).await;
        assert!(matches!(again, Err(WorkError::Conflict(_))));

        let restored = restore(&mut connection, id, Actor::User, minute(3))
            .await
            .expect("restore")
            .record;
        assert_eq!(restored.deleted_at, None);
        assert_eq!(restored.state, ActionItemState::Open);
        let listed = list(&mut connection, &everything, minute(4))
            .await
            .expect("list");
        assert_eq!(titles(&listed), ["tidy up"]);
        let not_deleted = restore(&mut connection, id, Actor::User, minute(4)).await;
        assert!(matches!(not_deleted, Err(WorkError::Conflict(_))));

        assert_eq!(
            history_kinds(&mut connection, id).await,
            ["created", "deleted", "restored"]
        );
        let unknown = restore(&mut connection, Uuid::now_v7(), Actor::User, minute(5)).await;
        assert!(matches!(
            unknown,
            Err(WorkError::Database(diesel::result::Error::NotFound))
        ));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn updates_record_only_the_fields_that_changed() {
        let (_url, mut connection) = migrated_database().await;
        let id = create_at(&mut connection, new_item("Reply to Sam"), minute(0))
            .await
            .id;

        let same = ActionItemChanges {
            title: Some("Reply to Sam".to_owned()),
            priority: Some(ActionItemPriority::Normal),
            ..ActionItemChanges::default()
        };
        let unchanged = update(&mut connection, id, same, Actor::User, minute(1))
            .await
            .expect("update");
        assert!(unchanged.history.is_empty(), "equal values change nothing");

        let edit = ActionItemChanges {
            title: Some("Reply to Sam about logs".to_owned()),
            priority: Some(ActionItemPriority::High),
            due_at: Some(Some(minute(60))),
            owner: Some(Owner::Other {
                name: "Sam".to_owned(),
            }),
            notes: Some(String::new()),
            ..ActionItemChanges::default()
        };
        let edited = update(&mut connection, id, edit, Actor::User, minute(2))
            .await
            .expect("update");
        assert_eq!(edited.record.priority, ActionItemPriority::High);
        assert_eq!(
            edited.record.owner(),
            Owner::Other {
                name: "Sam".to_owned()
            }
        );
        assert_eq!(
            edited.history[0].data,
            json!({ "changes": {
                "title": { "from": "Reply to Sam", "to": "Reply to Sam about logs" },
                "priority": { "from": "normal", "to": "high" },
                "dueAt": { "from": null, "to": minute(60) },
                "owner": { "from": { "kind": "user" }, "to": { "kind": "other", "name": "Sam" } },
            } })
        );

        let back_to_user = ActionItemChanges {
            owner: Some(Owner::User),
            due_at: Some(None),
            ..ActionItemChanges::default()
        };
        let reclaimed = update(&mut connection, id, back_to_user, Actor::User, minute(3))
            .await
            .expect("update")
            .record;
        assert_eq!(
            (reclaimed.owner_kind, reclaimed.owner_name),
            (OwnerKind::User, None)
        );
        assert_eq!(reclaimed.due_at, None);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn lists_filter_by_state_project_initiative_waiting_and_snooze() {
        let (_url, mut connection) = migrated_database().await;
        let now = minute(100);
        let project = project::create(
            &mut connection,
            &NewProject {
                name: "Elysium".to_owned(),
                description: String::new(),
            },
        )
        .await
        .expect("project");
        let launch = initiative::create(
            &mut connection,
            new_initiative("Launch"),
            Actor::User,
            minute(0),
        )
        .await
        .expect("initiative")
        .record;

        let in_project = NewActionItem {
            project_ids: vec![project.id],
            initiative_ids: vec![launch.id],
            ..new_item("in project")
        };
        create_at(&mut connection, in_project, minute(1)).await;
        let inbox = NewActionItem {
            state: ActionItemState::Inbox,
            ..new_item("inbox")
        };
        create_at(&mut connection, inbox, minute(2)).await;
        let waiting = create_at(&mut connection, new_item("waiting"), minute(3)).await;
        let wait = ActionItemChanges {
            waiting_on: Some(Some("Sam".to_owned())),
            ..ActionItemChanges::default()
        };
        change_at(&mut connection, waiting.id, wait, minute(4)).await;
        let snoozed = create_at(&mut connection, new_item("snoozed"), minute(5)).await;
        let snooze = ActionItemChanges {
            snoozed_until: Some(Some(now + TimeDelta::days(1))),
            ..ActionItemChanges::default()
        };
        change_at(&mut connection, snoozed.id, snooze, minute(6)).await;

        let cases = [
            (
                ActionItemFilter {
                    states: vec![ActionItemState::Inbox],
                    ..ActionItemFilter::default()
                },
                vec!["inbox"],
            ),
            (
                ActionItemFilter {
                    project: Some(ProjectFilter::Project(project.id)),
                    ..ActionItemFilter::default()
                },
                vec!["in project"],
            ),
            (
                ActionItemFilter {
                    project: Some(ProjectFilter::Unassigned),
                    ..ActionItemFilter::default()
                },
                vec!["snoozed", "waiting", "inbox"],
            ),
            (
                ActionItemFilter {
                    initiative: Some(launch.id),
                    ..ActionItemFilter::default()
                },
                vec!["in project"],
            ),
            (
                ActionItemFilter {
                    waiting: Some(true),
                    ..ActionItemFilter::default()
                },
                vec!["waiting"],
            ),
            (
                ActionItemFilter {
                    snoozed: Some(true),
                    ..ActionItemFilter::default()
                },
                vec!["snoozed"],
            ),
            (
                ActionItemFilter {
                    states: vec![ActionItemState::Open],
                    waiting: Some(false),
                    snoozed: Some(false),
                    ..ActionItemFilter::default()
                },
                vec!["in project"],
            ),
        ];
        for (filter, expected) in cases {
            let listed = list(&mut connection, &filter, now).await.expect("list");
            assert_eq!(titles(&listed), expected, "{filter:?}");
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn memberships_are_idempotent_and_leaving_keeps_the_span() {
        let (_url, mut connection) = migrated_database().await;
        let project = project::create(
            &mut connection,
            &NewProject {
                name: "Elysium".to_owned(),
                description: String::new(),
            },
        )
        .await
        .expect("project");
        let launch = initiative::create(
            &mut connection,
            new_initiative("Launch"),
            Actor::User,
            minute(0),
        )
        .await
        .expect("initiative")
        .record;
        let id = create_at(&mut connection, new_item("ship it"), minute(0))
            .await
            .id;

        let added = add_project(&mut connection, id, project.id, Actor::User, minute(1))
            .await
            .expect("add");
        assert_eq!(added.history.len(), 1);
        let again = add_project(&mut connection, id, project.id, Actor::User, minute(1))
            .await
            .expect("add again");
        assert!(again.history.is_empty(), "adding twice records once");
        let unknown =
            add_project(&mut connection, id, Uuid::now_v7(), Actor::User, minute(1)).await;
        assert!(matches!(
            unknown,
            Err(WorkError::Database(diesel::result::Error::NotFound))
        ));

        join_initiative(&mut connection, id, launch.id, Actor::User, minute(2))
            .await
            .expect("join");
        leave_initiative(&mut connection, id, launch.id, Actor::User, minute(3))
            .await
            .expect("leave");
        join_initiative(&mut connection, id, launch.id, Actor::User, minute(4))
            .await
            .expect("rejoin");
        let joined_twice = join_initiative(&mut connection, id, launch.id, Actor::User, minute(5))
            .await
            .expect("join again");
        assert!(joined_twice.history.is_empty());
        let left_elsewhere =
            leave_initiative(&mut connection, id, Uuid::now_v7(), Actor::User, minute(5))
                .await
                .expect("leave one it is not in");
        assert!(left_elsewhere.history.is_empty());

        let spans = initiative::memberships(&mut connection, &[launch.id])
            .await
            .expect("spans")
            .remove(&launch.id)
            .expect("the item's spans");
        let mut windows: Vec<_> = spans
            .iter()
            .map(|span| (span.joined_at, span.left_at))
            .collect();
        windows.sort_unstable();
        assert_eq!(
            windows,
            [(minute(2), Some(minute(3))), (minute(4), None)],
            "leaving closes a span and rejoining opens another"
        );

        let current = memberships(&mut connection, &[id])
            .await
            .expect("memberships");
        assert_eq!(current.project_ids(id), [project.id]);
        assert_eq!(current.initiative_ids(id), [launch.id]);

        let removed = remove_project(&mut connection, id, project.id, Actor::User, minute(6))
            .await
            .expect("remove");
        assert_eq!(removed.history.len(), 1);
        let removed_again = remove_project(&mut connection, id, project.id, Actor::User, minute(6))
            .await
            .expect("remove again");
        assert!(removed_again.history.is_empty());

        let membership_entries: Vec<(Option<Uuid>, String)> = list_for_item(&mut connection, id)
            .await
            .expect("history")
            .into_iter()
            .filter(|entry| entry.kind.starts_with("initiative_"))
            .map(|entry| (entry.initiative_id, entry.kind))
            .collect();
        assert_eq!(
            membership_entries,
            [
                (Some(launch.id), "initiative_joined".to_owned()),
                (Some(launch.id), "initiative_left".to_owned()),
                (Some(launch.id), "initiative_joined".to_owned()),
            ],
            "membership entries are about the item and the initiative both"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn creating_refuses_unknown_projects_and_deleted_initiatives_whole() {
        let (_url, mut connection) = migrated_database().await;
        let archived = initiative::create(
            &mut connection,
            new_initiative("Archived"),
            Actor::User,
            minute(0),
        )
        .await
        .expect("initiative")
        .record;
        initiative::soft_delete(&mut connection, archived.id, Actor::User, minute(1))
            .await
            .expect("delete");

        let into_deleted = NewActionItem {
            initiative_ids: vec![archived.id],
            ..new_item("into a deleted initiative")
        };
        let refused = create(&mut connection, into_deleted, Actor::User, minute(2)).await;
        assert!(matches!(refused, Err(WorkError::Invalid(_))));

        let into_unknown = NewActionItem {
            project_ids: vec![Uuid::now_v7()],
            ..new_item("into an unknown project")
        };
        let refused = create(&mut connection, into_unknown, Actor::User, minute(2)).await;
        assert!(matches!(
            refused,
            Err(WorkError::Database(diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::ForeignKeyViolation,
                _
            )))
        ));

        let everything = ActionItemFilter::default();
        let listed = list(&mut connection, &everything, minute(3))
            .await
            .expect("list");
        assert!(listed.is_empty(), "refused creates leave nothing behind");

        let fine = create_at(&mut connection, new_item("fine"), minute(3)).await;
        let join_deleted = join_initiative(
            &mut connection,
            fine.id,
            archived.id,
            Actor::User,
            minute(4),
        )
        .await;
        assert!(matches!(join_deleted, Err(WorkError::Conflict(_))));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn the_database_keeps_resolution_and_owner_columns_consistent() {
        let (_url, mut connection) = migrated_database().await;
        let id = create_at(&mut connection, new_item("consistent"), minute(0))
            .await
            .id;

        diesel::update(action_items::table.find(id))
            .set(action_items::resolved_at.eq(Some(minute(1))))
            .execute(&mut connection)
            .await
            .expect_err("action_items_resolved_at_set refuses a resolution while open");
        diesel::update(action_items::table.find(id))
            .set(action_items::owner_kind.eq(OwnerKind::Other))
            .execute(&mut connection)
            .await
            .expect_err("action_items_owner_name_set requires another owner's name");
        diesel::update(action_items::table.find(id))
            .set(action_items::owner_name.eq(Some("Sam")))
            .execute(&mut connection)
            .await
            .expect_err("action_items_owner_name_set refuses a name for the user");
    }
}
