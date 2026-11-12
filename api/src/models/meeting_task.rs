// Copyright © 2026 Jalapeno Labs

//! The tasks heard in a meeting, and the action items each was made into. See `docs/meetings.md`,
//! Tasks.
//!
//! A task is a mark of kind `task` over the words that say it (`models::meeting_mark`): the mark
//! anchors it, moves with an edit, and stands or is removed as every mark does. What the task is
//! (what is to be done, who owes it, the project it is for) is its row here, keyed by its mark, as a
//! thread's comments sit beside their mark. So is why its project's Project Manager made no item of
//! it, when it said so. The items made of a task are `meeting_task_items`, one row an item, so the
//! meeting's bar opens a task's items and each item's page opens its task's range. Who segmenting
//! reads a task for, and what is done with it, is `meetings::tasks`'.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::schema::{
    action_items, changeset_operations, changesets, meeting_marks, meeting_task_items,
    meeting_tasks,
};
use crate::models::action_item::ActionItem;
use crate::models::changeset::ChangesetState;

/// The longest title a task takes, in characters, as `meeting_tasks.title` allows.
pub const TITLE_MAX_CHARACTERS: usize = 200;

/// The longest name of someone not on Elysium who owes a task, in characters.
pub const NAME_MAX_CHARACTERS: usize = 200;

/// The longest reason a Project Manager gives for making no item of a task, in characters.
pub const DECLINE_REASON_MAX_CHARACTERS: usize = 2_000;

/// Who owes a task.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum OwedBy {
    /// A person on Elysium, the meeting's own person or a coworker.
    Person { user_id: Uuid },
    /// Someone the meeting names who is not on Elysium, by that name.
    Named { name: String },
    /// Nobody was named.
    Nobody,
}

/// What a task is, as segmenting read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskReading {
    /// What is to be done, as a short phrase.
    pub title: String,
    pub owed_by: OwedBy,
    /// The project it is for; `None` for a task no project is about.
    pub project_id: Option<Uuid>,
}

/// One stored task.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = meeting_tasks, check_for_backend(diesel::pg::Pg))]
pub struct MeetingTask {
    /// Its mark, which is its id.
    pub mark_id: Uuid,
    pub meeting_id: Uuid,
    pub title: String,
    pub owed_by_user_id: Option<Uuid>,
    pub owed_by_name: Option<String>,
    pub project_id: Option<Uuid>,
    /// Why its project's Project Manager made no item of it.
    pub declined_reason: Option<String>,
    pub declined_at: Option<DateTime<Utc>>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

impl MeetingTask {
    /// Who owes it.
    pub fn owed_by(&self) -> OwedBy {
        if let Some(user_id) = self.owed_by_user_id {
            return OwedBy::Person { user_id };
        }
        match &self.owed_by_name {
            Some(name) => OwedBy::Named { name: name.clone() },
            None => OwedBy::Nobody,
        }
    }

    /// What it is, as a reading of it would say.
    pub fn reading(&self) -> TaskReading {
        TaskReading {
            title: self.title.clone(),
            owed_by: self.owed_by(),
            project_id: self.project_id,
        }
    }
}

/// The `owed_by_user_id` and `owed_by_name` columns for `owed_by`.
fn owed_by_columns(owed_by: &OwedBy) -> (Option<Uuid>, Option<String>) {
    match owed_by {
        OwedBy::Person { user_id } => (Some(*user_id), None),
        OwedBy::Named { name } => (None, Some(name.clone())),
        OwedBy::Nobody => (None, None),
    }
}

/// Stores task mark `mark_id` of meeting `meeting_id` as `reading`, made by `created_by`. The
/// caller made its mark in the same transaction, under the meeting's lock.
///
/// # Errors
/// Propagates any database error.
pub async fn insert(
    connection: &mut AsyncPgConnection,
    mark_id: Uuid,
    meeting_id: Uuid,
    reading: &TaskReading,
    created_by: Uuid,
) -> QueryResult<MeetingTask> {
    let (owed_by_user_id, owed_by_name) = owed_by_columns(&reading.owed_by);
    diesel::insert_into(meeting_tasks::table)
        .values((
            meeting_tasks::mark_id.eq(mark_id),
            meeting_tasks::meeting_id.eq(meeting_id),
            meeting_tasks::title.eq(&reading.title),
            meeting_tasks::owed_by_user_id.eq(owed_by_user_id),
            meeting_tasks::owed_by_name.eq(owed_by_name),
            meeting_tasks::project_id.eq(reading.project_id),
            meeting_tasks::created_by.eq(created_by),
        ))
        .returning(MeetingTask::as_returning())
        .get_result(connection)
        .await
}

/// Gives task `mark_id` what `reading` says it is, as a meeting read again said.
///
/// # Errors
/// Propagates any database error.
pub async fn set_reading(
    connection: &mut AsyncPgConnection,
    mark_id: Uuid,
    reading: &TaskReading,
) -> QueryResult<MeetingTask> {
    let (owed_by_user_id, owed_by_name) = owed_by_columns(&reading.owed_by);
    diesel::update(meeting_tasks::table.find(mark_id))
        .set((
            meeting_tasks::title.eq(&reading.title),
            meeting_tasks::owed_by_user_id.eq(owed_by_user_id),
            meeting_tasks::owed_by_name.eq(owed_by_name),
            meeting_tasks::project_id.eq(reading.project_id),
        ))
        .returning(MeetingTask::as_returning())
        .get_result(connection)
        .await
}

/// Meeting `meeting_id`'s tasks whose marks stand, by mark id.
///
/// # Errors
/// Propagates any database error.
pub async fn standing_of(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> QueryResult<HashMap<Uuid, MeetingTask>> {
    let tasks: Vec<MeetingTask> = meeting_tasks::table
        .inner_join(meeting_marks::table)
        .filter(meeting_tasks::meeting_id.eq(meeting_id))
        .filter(meeting_marks::removed_by.is_null())
        .select(MeetingTask::as_select())
        .load(connection)
        .await?;
    Ok(tasks.into_iter().map(|task| (task.mark_id, task)).collect())
}

/// The tasks among `ids` that are for `project_id` and whose marks still stand, oldest first: a
/// task read again onto another project, or no longer heard at all, is not that project's to act
/// on.
///
/// # Errors
/// Propagates any database error.
pub async fn standing_for_project(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    ids: &[Uuid],
) -> QueryResult<Vec<MeetingTask>> {
    meeting_tasks::table
        .inner_join(meeting_marks::table)
        .filter(meeting_tasks::mark_id.eq_any(ids))
        .filter(meeting_tasks::project_id.eq(project_id))
        .filter(meeting_marks::removed_by.is_null())
        .order(meeting_tasks::mark_id.asc())
        .select(MeetingTask::as_select())
        .load(connection)
        .await
}

/// Records why task `mark_id`'s Project Manager made no item of it, at `at`, replacing any reason
/// it gave before.
///
/// # Errors
/// Propagates any database error.
pub async fn decline(
    connection: &mut AsyncPgConnection,
    mark_id: Uuid,
    reason: &str,
    at: DateTime<Utc>,
) -> QueryResult<MeetingTask> {
    diesel::update(meeting_tasks::table.find(mark_id))
        .set((
            meeting_tasks::declined_reason.eq(reason),
            meeting_tasks::declined_at.eq(at),
        ))
        .returning(MeetingTask::as_returning())
        .get_result(connection)
        .await
}

/// Records that action item `item_id` was made of task `task_id`, for `person_id` (whose it is, or
/// `None` for a project's), by `created_by`. Call it in the transaction that creates the item.
///
/// # Errors
/// Propagates any database error.
pub async fn record_item(
    connection: &mut AsyncPgConnection,
    task_id: Uuid,
    item_id: Uuid,
    person_id: Option<Uuid>,
    created_by: Uuid,
) -> QueryResult<()> {
    diesel::insert_into(meeting_task_items::table)
        .values((
            meeting_task_items::action_item_id.eq(item_id),
            meeting_task_items::task_id.eq(task_id),
            meeting_task_items::person_id.eq(person_id),
            meeting_task_items::created_by.eq(created_by),
        ))
        .execute(connection)
        .await
        .map(drop)
}

/// Whether task `task_id` was already made into an item for `person_id`, or by its project's
/// Project Manager, deleted or not: a meeting read again never raises a second one for the same
/// person, nor one beside the item a project made of a task later read onto no project.
///
/// # Errors
/// Propagates any database error.
pub async fn has_item_for(
    connection: &mut AsyncPgConnection,
    task_id: Uuid,
    person_id: Uuid,
) -> QueryResult<bool> {
    diesel::select(diesel::dsl::exists(
        meeting_task_items::table
            .filter(meeting_task_items::task_id.eq(task_id))
            .filter(
                meeting_task_items::person_id
                    .eq(person_id)
                    .or(meeting_task_items::person_id.is_null()),
            ),
    ))
    .get_result(connection)
    .await
}

/// The live action items made of each of `task_ids`, oldest first, by task.
///
/// # Errors
/// Propagates any database error.
pub async fn items_of(
    connection: &mut AsyncPgConnection,
    task_ids: &[Uuid],
) -> QueryResult<HashMap<Uuid, Vec<ActionItem>>> {
    let rows: Vec<(Uuid, ActionItem)> = meeting_task_items::table
        .inner_join(action_items::table)
        .filter(meeting_task_items::task_id.eq_any(task_ids))
        .filter(action_items::deleted_at.is_null())
        .order(action_items::id.asc())
        .select((meeting_task_items::task_id, ActionItem::as_select()))
        .load(connection)
        .await?;
    let mut by_task: HashMap<Uuid, Vec<ActionItem>> = HashMap::new();
    for (task_id, item) in rows {
        by_task.entry(task_id).or_default().push(item);
    }
    Ok(by_task)
}

/// The changesets still waiting for a person that would make an item of each of `task_ids`, by
/// task, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn pending_proposals_of(
    connection: &mut AsyncPgConnection,
    task_ids: &[Uuid],
) -> QueryResult<HashMap<Uuid, Vec<Uuid>>> {
    let rows: Vec<(Option<Uuid>, Uuid)> = changeset_operations::table
        .inner_join(changesets::table)
        .filter(changeset_operations::meeting_task_id.eq_any(task_ids))
        .filter(changesets::state.eq(ChangesetState::Pending))
        .order(changesets::id.asc())
        .select((changeset_operations::meeting_task_id, changesets::id))
        .load(connection)
        .await?;
    let mut by_task: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for (task_id, changeset_id) in rows {
        let Some(task_id) = task_id else {
            continue;
        };
        let proposals = by_task.entry(task_id).or_default();
        if !proposals.contains(&changeset_id) {
            proposals.push(changeset_id);
        }
    }
    Ok(by_task)
}

/// The meeting and the task an action item was made of, if a task made it.
///
/// # Errors
/// Propagates any database error.
pub async fn task_of_item(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
) -> QueryResult<Option<(Uuid, Uuid)>> {
    meeting_task_items::table
        .inner_join(meeting_tasks::table)
        .filter(meeting_task_items::action_item_id.eq(action_item_id))
        .select((meeting_tasks::meeting_id, meeting_tasks::mark_id))
        .first(connection)
        .await
        .optional()
}
