// Copyright © 2026 Jalapeno Labs

//! The one inbox item per project that asks a person to approve its proposed batches.
//!
//! A project's Project Manager may propose many batches in one turn, so the person is told once
//! per project, never once per batch: an item titled with how many wait, in the project, kept up
//! to date as batches are proposed and decided, and resolved once none are left. Which item a
//! project has is a row of `work_batch_approval_items`.
//!
//! Every write that moves a batch into or out of `proposed` calls [`refresh`] in its own
//! transaction, so the item never disagrees with the batches it counts. A person who resolves or
//! dismisses the item by hand is done with it: deciding the batches that remain changes nothing,
//! and the next batch proposed raises a new one.
//!
//! The item names the project and the count alone, never a batch's title: a batch quotes what was
//! ingested, so its words are for the company's members, and the work queue and the project's page
//! show them there.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use super::BatchState;
use crate::action_items::{Actor, Transition, WorkError};
use crate::database::schema::{work_batch_approval_items, work_batches};
use crate::models::action_item::{
    self, ActionItem, ActionItemChanges, ActionItemPriority, ActionItemState, NewActionItem, Owner,
};
use crate::models::action_item_event::Recorded;
use crate::models::project;
use crate::models::user::{ELYSIA_USER_ID, PROJECT_MANAGER_USER_ID, SYSTEM_USER_ID};

/// Serializes every refresh of one project's item, in whichever process makes it, so two writes
/// deciding at once never both raise one. Hashed with the project's id.
const LOCK_PREFIX: &str = "work_batches.approval_item:";

/// Why a project's item is refreshed: a batch was proposed, which may need an item raised, or a
/// proposed batch was decided or withdrawn, which only ever updates or resolves the one there is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    Proposed,
    Settled,
}

/// Brings `project_id`'s approval item in line with how many of its batches are proposed, at
/// `now`, as `actor`: raises one when a batch was just proposed and none is open, retitles the open
/// one when the count moved, and resolves it once none are left. Call it in the transaction that
/// moved a batch into or out of `proposed`. Answers the item it wrote, for the caller to announce
/// once that transaction commits.
///
/// # Errors
/// Propagates any database error.
///
/// # Panics
/// When an item's own rules refuse a write to an item this function has just locked and found
/// live, which would be a programming error.
pub async fn refresh(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    cause: Cause,
    actor: Actor,
    now: DateTime<Utc>,
) -> QueryResult<Option<Recorded<ActionItem>>> {
    diesel::sql_query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind::<diesel::sql_types::Text, _>(format!("{LOCK_PREFIX}{project_id}"))
        .execute(connection)
        .await?;
    let waiting: i64 = work_batches::table
        .filter(work_batches::project_id.eq(project_id))
        .filter(work_batches::state.eq(BatchState::Proposed))
        .count()
        .get_result(connection)
        .await?;
    let open = open_item(connection, project_id).await?;

    let written = match (open, waiting) {
        (None, 0) => return Ok(None),
        (None, _) if cause == Cause::Settled => return Ok(None),
        (Some(item), 0) => {
            action_item::transition(connection, item.id, Transition::Resolve, actor, now).await
        }
        (Some(item), _) => {
            let name = project::find(connection, project_id).await?.name;
            let wording = new_item(&name, project_id, waiting);
            let changes = ActionItemChanges {
                title: Some(wording.title),
                notes: Some(wording.notes),
                ..ActionItemChanges::default()
            };
            action_item::update(connection, item.id, changes, actor, now).await
        }
        (None, _) => {
            let name = project::find(connection, project_id).await?.name;
            let wording = new_item(&name, project_id, waiting);
            let raised = action_item::create(connection, wording, actor, now)
                .await
                .map_err(database_only)?;
            remember(connection, project_id, raised.record.id, actor).await?;
            Ok(raised)
        }
    };
    written.map(Some).map_err(database_only)
}

/// The actor an approval item's history names for a change `by` made to one of `project_id`'s
/// batches: the project's Project Manager, Elysia, Elysium itself, or the person.
pub const fn actor_for(by: Uuid, project_id: Uuid) -> Actor {
    match by {
        PROJECT_MANAGER_USER_ID => Actor::ProjectManager(project_id),
        ELYSIA_USER_ID => Actor::Elysia,
        SYSTEM_USER_ID => Actor::System,
        person => Actor::User(person),
    }
}

/// `project_id`'s item while it is still open to act on: in the inbox or accepted, and not
/// deleted, locked until the transaction ends. `None` when it has none, or its person dealt with
/// the one it had.
async fn open_item(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
) -> QueryResult<Option<ActionItem>> {
    let item_id: Option<Uuid> = work_batch_approval_items::table
        .find(project_id)
        .select(work_batch_approval_items::action_item_id)
        .first(connection)
        .await
        .optional()?;
    let Some(item_id) = item_id else {
        return Ok(None);
    };
    match action_item::lock_live(connection, item_id).await {
        Ok(item) if matches!(item.state, ActionItemState::Inbox | ActionItemState::Open) => {
            Ok(Some(item))
        }
        // Resolved, dismissed, or deleted by its person: theirs to have closed.
        Ok(_) | Err(WorkError::Conflict(_)) => Ok(None),
        Err(error) => Err(database_only(error)),
    }
}

/// Records `item_id` as `project_id`'s item, replacing the one its person closed.
async fn remember(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    item_id: Uuid,
    actor: Actor,
) -> QueryResult<()> {
    diesel::insert_into(work_batch_approval_items::table)
        .values((
            work_batch_approval_items::project_id.eq(project_id),
            work_batch_approval_items::action_item_id.eq(item_id),
            work_batch_approval_items::created_by.eq(actor.user_id()),
        ))
        .on_conflict(work_batch_approval_items::project_id)
        .do_update()
        .set(work_batch_approval_items::action_item_id.eq(item_id))
        .execute(connection)
        .await
        .map(drop)
}

/// The item for `waiting` proposed batches of the project named `name`, whose words an open item
/// is kept to as well.
fn new_item(name: &str, project_id: Uuid, waiting: i64) -> NewActionItem {
    NewActionItem {
        title: title(name, waiting),
        notes: format!(
            "{name}'s Project Manager proposed work that no worker starts until a person approves \
             it. Approve, edit, or reject each batch under Proposed, on the work queue or the \
             project's page."
        ),
        state: ActionItemState::Inbox,
        priority: ActionItemPriority::Normal,
        due_at: None,
        owner: Owner::User,
        project_ids: vec![project_id],
        initiative_ids: Vec::new(),
    }
}

/// What the item says: how many batches of the project named `name` wait for approval.
fn title(name: &str, waiting: i64) -> String {
    if waiting == 1 {
        return format!("Approve a work batch for {name}");
    }
    format!("Approve {waiting} work batches for {name}")
}

/// The database error inside `error`. The item's own rules cannot refuse what this module writes:
/// it locks the item and checks it is open before updating or resolving it, and raises one with no
/// initiative.
fn database_only(error: WorkError) -> diesel::result::Error {
    match error {
        WorkError::Database(error) => error,
        refused => panic!("an approval item's write was refused by the item's rules: {refused}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_counts_what_waits() {
        assert_eq!(title("Launch", 1), "Approve a work batch for Launch");
        assert_eq!(title("Launch", 3), "Approve 3 work batches for Launch");
    }

    #[test]
    fn each_machine_acts_as_itself_and_anyone_else_as_a_person() {
        let project = Uuid::now_v7();
        let person = Uuid::now_v7();
        assert_eq!(
            actor_for(PROJECT_MANAGER_USER_ID, project),
            Actor::ProjectManager(project)
        );
        assert_eq!(actor_for(ELYSIA_USER_ID, project), Actor::Elysia);
        assert_eq!(actor_for(SYSTEM_USER_ID, project), Actor::System);
        assert_eq!(actor_for(person, project), Actor::User(person));
    }
}
