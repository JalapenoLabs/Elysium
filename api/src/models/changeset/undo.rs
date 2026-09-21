// Copyright © 2026 Jalapeno Labs

//! Undoing an applied changeset: every applied operation is reversed, last first, as the
//! user.
//!
//! What each operation reverses to:
//!
//! | Operation              | Undo                                                          |
//! |------------------------|---------------------------------------------------------------|
//! | create an item         | Deletes it softly, so it can still be restored                |
//! | update an item         | Puts back each field that still holds what the update wrote   |
//! | resolve or dismiss     | Returns the item to where it was, if it has not moved since   |
//! | comment                | Takes the comment back                                        |
//! | link                   | Removes the link, if the operation added it                   |
//! | add to an initiative   | Takes the item out again, if the operation put it in          |
//! | remove from one        | Puts it back, if the operation took it out                    |
//! | create an initiative   | Deletes it softly                                             |
//!
//! Some effects have already left Elysium and cannot be reversed from here: a comment the
//! watcher already posted to the provider stays there, and an issue the watcher already
//! closed stays closed when its item returns. Each operation's `undo` records what was not
//! reversed so the review says so. Changes the user made after the changeset are never
//! overwritten: a field edited since keeps the user's value, and an item moved since stays
//! where the user put it.
//!
//! Like applying, the whole undo runs in one transaction and each operation in a savepoint,
//! so one the records refuse (its item deleted since) is kept, with the reason, and the rest
//! are reversed.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::{
    ChangesetOutcome, ChangesetState, Staged, Touched, Written, conclude, lock, operations_of,
};
use crate::action_items::changesets::{self, Operation};
use crate::action_items::{Actor, WorkError};
use crate::database::schema::changeset_operations;
use crate::models::action_item::{self, ActionItemChanges, ActionItemState};
use crate::models::action_item_event;
use crate::models::action_item_link::{self, LinkKind, LinkState};
use crate::models::{action_item_comment, action_item_link_write, initiative};

/// Why one operation could not be reversed.
#[derive(Debug)]
enum Failure {
    /// The records refuse it, in words for the user; the rest of the undo goes ahead.
    Refused(String),
    /// A fault inside Elysium, which rolls the whole undo back.
    Fault(DieselError),
}

impl From<DieselError> for Failure {
    fn from(error: DieselError) -> Self {
        match error {
            DieselError::NotFound => Self::Refused("what it names no longer exists".to_owned()),
            other => Self::Fault(other),
        }
    }
}

impl From<WorkError> for Failure {
    fn from(error: WorkError) -> Self {
        match error {
            WorkError::Database(database) => database.into(),
            WorkError::Conflict(message) | WorkError::Invalid(message) => {
                Self::Refused(message.to_owned())
            }
            WorkError::Refused(message) => Self::Refused(message),
        }
    }
}

/// What undoing one operation did.
#[derive(Debug, Default)]
struct Reversed {
    /// Whether the operation's effect is gone from Elysium.
    undone: bool,
    /// What could not be reversed, for the review: see [`undo`].
    details: Map<String, Value>,
    touched: Touched,
}

impl Reversed {
    fn done() -> Self {
        Self {
            undone: true,
            ..Self::default()
        }
    }
}

/// Reverses every applied operation of an applied changeset, last first, and marks it
/// `undone`. See the module docs for what each operation reverses to and what cannot be.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown changeset,
/// [`WorkError::Conflict`] for one that is not applied, and any database fault, which
/// reverses nothing.
pub async fn undo(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<Written, WorkError> {
    connection
        .transaction(async move |connection| {
            let changeset = lock(connection, id).await?;
            if changeset.state != ChangesetState::Applied {
                return Err(WorkError::Conflict(
                    "only an applied changeset can be undone, and only once",
                ));
            }
            let operations = operations_of(connection, &[id])
                .await?
                .remove(&id)
                .unwrap_or_default();

            let mut touched = Touched::default();
            for row in operations.iter().rev() {
                if row.outcome != ChangesetOutcome::Applied {
                    continue;
                }
                let operation = row.parsed();
                let result = row.result.clone();
                let attempt = connection
                    .transaction(async move |connection| {
                        let mut reversed = reverse(connection, &operation, &result, now).await?;
                        let history = std::mem::take(&mut reversed.touched.history);
                        reversed.touched.history =
                            action_item_event::attribute(connection, history, id).await?;
                        Ok::<_, Failure>(reversed)
                    })
                    .await;
                let (outcome, details) = match attempt {
                    Ok(reversed) => {
                        touched.absorb(reversed.touched);
                        let outcome = if reversed.undone {
                            ChangesetOutcome::Undone
                        } else {
                            ChangesetOutcome::Applied
                        };
                        (outcome, reversed.details)
                    }
                    Err(Failure::Refused(message)) => {
                        let mut details = Map::new();
                        details.insert("refusal".to_owned(), json!(message));
                        (ChangesetOutcome::Applied, details)
                    }
                    Err(Failure::Fault(error)) => return Err(WorkError::Database(error)),
                };
                diesel::update(changeset_operations::table.find(row.id))
                    .set((
                        changeset_operations::outcome.eq(outcome),
                        changeset_operations::undo.eq(Value::Object(details)),
                    ))
                    .execute(connection)
                    .await?;
            }

            let changeset = conclude(connection, id, ChangesetState::Undone, now).await?;
            let operations = operations_of(connection, &[id])
                .await?
                .remove(&id)
                .unwrap_or_default();
            Ok(Written {
                staged: Staged {
                    changeset,
                    operations,
                },
                touched,
            })
        })
        .await
}

/// An id `result` holds under `key`.
///
/// # Panics
/// Panics when it holds none: applying wrote it for every operation of that kind.
fn result_id(result: &Value, key: &str) -> Uuid {
    serde_json::from_value(result[key].clone())
        .expect("an applied operation's result names what it acted on")
}

/// Reverses one applied operation as the user.
async fn reverse(
    connection: &mut AsyncPgConnection,
    operation: &Operation,
    result: &Value,
    now: DateTime<Utc>,
) -> Result<Reversed, Failure> {
    match operation {
        Operation::CreateItem(_) => delete_item(connection, result, now).await,
        Operation::UpdateItem(_) => restore_fields(connection, result, now).await,
        Operation::ResolveItem(_) => {
            return_item(connection, result, ActionItemState::Resolved, now).await
        }
        Operation::DismissItem(_) => {
            return_item(connection, result, ActionItemState::Dismissed, now).await
        }
        Operation::Comment(_) => withdraw_comment(connection, result, now).await,
        Operation::Link(_) => remove_link(connection, result, now).await,
        Operation::AddToInitiative(_) => reverse_membership(connection, result, false, now).await,
        Operation::RemoveFromInitiative(_) => {
            reverse_membership(connection, result, true, now).await
        }
        Operation::CreateInitiative(_) => delete_initiative(connection, result, now).await,
    }
}

/// Deletes the item a create made, softly, so it can still be restored. One the user
/// deleted since is already where an undo would put it.
async fn delete_item(
    connection: &mut AsyncPgConnection,
    result: &Value,
    now: DateTime<Utc>,
) -> Result<Reversed, Failure> {
    let id = result_id(result, "itemId");
    let item = action_item::find(connection, id).await?;
    let mut reversed = Reversed::done();
    if item.deleted_at.is_none() {
        let deleted = action_item::soft_delete(connection, id, Actor::User, now).await?;
        reversed.touched.history.extend(deleted.history);
        reversed.touched.item_ids.insert(id);
    }
    Ok(reversed)
}

/// Puts back each field an update replaced that still holds what the update wrote, and
/// names the ones the user changed since, which keep the user's value.
async fn restore_fields(
    connection: &mut AsyncPgConnection,
    result: &Value,
    now: DateTime<Utc>,
) -> Result<Reversed, Failure> {
    let id = result_id(result, "itemId");
    let item = action_item::find(connection, id).await?;
    let current = json!({
        "title": item.title,
        "notes": item.notes,
        "priority": item.priority,
        "dueAt": item.due_at,
    });
    let changes = result["changes"].as_object().cloned().unwrap_or_default();
    let restorable =
        changesets::restorable_fields(&changes, current.as_object().expect("built as an object"));
    let mut reversed = Reversed {
        // An update that changed nothing has nothing to reverse.
        undone: !restorable.restore.is_empty() || changes.is_empty(),
        ..Reversed::default()
    };
    if !restorable.kept.is_empty() {
        reversed
            .details
            .insert("kept".to_owned(), json!(restorable.kept));
    }
    if restorable.restore.is_empty() {
        return Ok(reversed);
    }

    let changes = ActionItemChanges {
        title: restored(&restorable.restore, "title"),
        notes: restored(&restorable.restore, "notes"),
        priority: restored(&restorable.restore, "priority"),
        due_at: restored(&restorable.restore, "dueAt"),
        ..ActionItemChanges::default()
    };
    let updated = action_item::update(connection, id, changes, Actor::User, now).await?;
    reversed.touched.history.extend(updated.history);
    reversed.touched.item_ids.insert(id);
    Ok(reversed)
}

/// The value `restore` holds for `field`, as the item's field type.
///
/// # Panics
/// Panics when the value is not of that type. History wrote each one from the item's own
/// field, so it reads back.
fn restored<Field: DeserializeOwned>(restore: &Map<String, Value>, field: &str) -> Option<Field> {
    restore.get(field).cloned().map(|value| {
        serde_json::from_value(value).expect("history records each field as the item holds it")
    })
}

/// Returns an item a changeset resolved or dismissed to the state it left, unless the user
/// moved it since. After a resolve, names the linked issues the watcher already closed,
/// which stay closed.
async fn return_item(
    connection: &mut AsyncPgConnection,
    result: &Value,
    reached: ActionItemState,
    now: DateTime<Utc>,
) -> Result<Reversed, Failure> {
    let id = result_id(result, "itemId");
    let from: ActionItemState = serde_json::from_value(result["from"].clone())
        .expect("a transition's result holds the state it left");
    let item = action_item::find(connection, id).await?;
    if item.deleted_at.is_some() {
        return Err(Failure::Refused(
            "the item is deleted; restore it first".to_owned(),
        ));
    }
    let mut reversed = Reversed::default();
    if item.state != reached {
        reversed
            .details
            .insert("movedSince".to_owned(), json!(true));
        return Ok(reversed);
    }

    if reached == ActionItemState::Resolved {
        let still_closed = closed_issues(connection, id).await?;
        if !still_closed.is_empty() {
            reversed
                .details
                .insert("stillClosed".to_owned(), json!(still_closed));
        }
    }

    let returned = action_item::return_to(connection, id, from, Actor::User, now).await?;
    reversed.undone = true;
    reversed.touched.history.extend(returned.history);
    reversed.touched.item_ids.insert(id);
    reversed.touched.link_item_ids.insert(id);
    Ok(reversed)
}

/// The item's linked issues that are done with no close still owed: the ones the watcher
/// already closed, or that were done anyway. Returning the item leaves them closed.
async fn closed_issues(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> Result<Vec<Value>, Failure> {
    let links = action_item_link::list_for_item(connection, id).await?;
    let link_ids: Vec<Uuid> = links.iter().map(|link| link.id).collect();
    let owed = action_item_link_write::for_links(connection, &link_ids).await?;
    Ok(links
        .iter()
        .filter(|link| link.kind == LinkKind::Issue && link.observed_state == LinkState::Done)
        .filter(|link| !owed.iter().any(|write| write.link_id == link.id))
        .map(|link| json!({ "provider": link.provider, "key": link.external_key }))
        .collect())
}

/// Takes back a comment a changeset wrote. A post to the provider that has not landed is
/// dropped with it; one that landed stays, and is named.
async fn withdraw_comment(
    connection: &mut AsyncPgConnection,
    result: &Value,
    now: DateTime<Utc>,
) -> Result<Reversed, Failure> {
    let id = result_id(result, "itemId");
    let comment_id = result_id(result, "commentId");
    let mut reversed = Reversed::done();
    let still_owed = action_item_link_write::for_comment(connection, comment_id)
        .await?
        .is_some();
    let owed_to: Option<Uuid> = serde_json::from_value(result["linkId"].clone())
        .expect("a comment's result names the link it was owed to, or null");
    // Owed to a link and no longer owed: the watcher posted it, and it stays posted.
    if let Some(link_id) = owed_to.filter(|_link_id| !still_owed)
        && let Some(link) = action_item_link::find(connection, link_id)
            .await
            .optional()?
    {
        reversed.details.insert(
            "stillPosted".to_owned(),
            json!({ "provider": link.provider, "key": link.external_key }),
        );
    }

    let exists = action_item_comment::find(connection, comment_id)
        .await
        .optional()?
        .is_some();
    // A comment deleted since is already gone.
    if exists {
        let withdrawn =
            action_item_comment::withdraw(connection, id, comment_id, Actor::User, now).await?;
        reversed.touched.history.extend(withdrawn.history);
        reversed.touched.withdrawn_comments.push(withdrawn.record);
        reversed.touched.link_item_ids.insert(id);
    }
    Ok(reversed)
}

/// Removes a link the changeset added. One that was there already, or that the user removed
/// since, is left alone.
async fn remove_link(
    connection: &mut AsyncPgConnection,
    result: &Value,
    now: DateTime<Utc>,
) -> Result<Reversed, Failure> {
    let mut reversed = Reversed::done();
    if result["added"] != json!(true) {
        return Ok(reversed);
    }
    let id = result_id(result, "itemId");
    let link_id = result_id(result, "linkId");
    let Some(link) = action_item_link::find(connection, link_id)
        .await
        .optional()?
    else {
        return Ok(reversed);
    };
    let removed = action_item_link::remove(connection, id, link_id, Actor::User, now).await?;
    reversed.touched.history.extend(removed.item.history);
    reversed.touched.item_ids.insert(id);
    reversed.touched.link_item_ids.insert(id);
    reversed.touched.removed_links.push(link);
    Ok(reversed)
}

/// Takes an item back out of the initiative the changeset put it in (`joins` false), or
/// puts it back into one the changeset took it out of, when the changeset changed anything.
async fn reverse_membership(
    connection: &mut AsyncPgConnection,
    result: &Value,
    joins: bool,
    now: DateTime<Utc>,
) -> Result<Reversed, Failure> {
    let mut reversed = Reversed::done();
    if result["changed"] != json!(true) {
        return Ok(reversed);
    }
    let id = result_id(result, "itemId");
    let initiative_id = result_id(result, "initiativeId");
    let written = if joins {
        action_item::join_initiative(connection, id, initiative_id, Actor::User, now).await?
    } else {
        action_item::leave_initiative(connection, id, initiative_id, Actor::User, now).await?
    };
    reversed.touched.history.extend(written.history);
    reversed.touched.item_ids.insert(id);
    reversed.touched.initiative_ids.insert(initiative_id);
    Ok(reversed)
}

/// Deletes the initiative a create made, softly. One the user deleted since stays as it is.
async fn delete_initiative(
    connection: &mut AsyncPgConnection,
    result: &Value,
    now: DateTime<Utc>,
) -> Result<Reversed, Failure> {
    let id = result_id(result, "initiativeId");
    let found = initiative::find(connection, id).await?;
    let mut reversed = Reversed::done();
    if found.deleted_at.is_none() {
        let deleted = initiative::soft_delete(connection, id, Actor::User, now).await?;
        reversed.touched.history.extend(deleted.history);
        reversed.touched.initiative_ids.insert(id);
    }
    Ok(reversed)
}
