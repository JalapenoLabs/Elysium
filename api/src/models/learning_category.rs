// Copyright © 2026 Jalapeno Labs

//! The categories learnings are filed under: Elysium's nine built-in ones and the workspace's
//! own (`docs/knowledge.md`, Categories).
//!
//! Every category is a row of `learning_categories`, and `learnings.category` references its
//! key, so Postgres itself keeps a learning from outliving its category. The built-in rows are
//! seeded by their migration and never change; admins add, edit, reorder, and remove the rest.
//! A key never changes, since learnings and their history hold it.
//!
//! The list is a workspace setting, so each change is recorded in `user_events` beside the
//! other workspace settings, with its actor and the values before and after. Removing a category
//! that learnings are filed under moves them to another first, each move a revision in that
//! learning's own history, so none is left without a category and each can be undone.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::database::schema::{learning_categories, learnings};
use crate::models::agent::Author;
use crate::models::learning::{self, LearningError, Recorded};
use crate::models::user::{self, UserEventKind};

/// A stored category.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(
    table_name = learning_categories,
    primary_key(key),
    check_for_backend(diesel::pg::Pg)
)]
pub struct LearningCategory {
    /// What learnings store and agents send. Never changes.
    pub key: String,
    pub label: String,
    /// What belongs there, for the agent choosing a category.
    pub description: String,
    /// Its place among the categories of its kind: built-in ones are always offered first.
    pub position: i32,
    /// One of Elysium's nine, which cannot be changed or removed.
    pub built_in: bool,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl LearningCategory {
    /// What history records of a category: everything an admin can set, and its key.
    fn recorded(&self) -> Value {
        json!({
            "key": self.key,
            "label": self.label,
            "description": self.description,
            "position": self.position,
        })
    }
}

/// A category to add. Its key, label, and description are already trimmed and checked.
#[derive(Debug, Clone)]
pub struct NewCategory {
    pub key: String,
    pub label: String,
    pub description: String,
}

/// An edit of a workspace category: each field given replaces its own.
#[derive(Debug, Default, Clone)]
pub struct CategoryChanges {
    pub label: Option<String>,
    pub description: Option<String>,
    /// Where it goes among the workspace's own categories, counted from 0; past the end puts
    /// it last.
    pub position: Option<usize>,
}

/// Why a change to the categories was refused.
#[derive(Debug, thiserror::Error)]
pub enum CategoryError {
    /// Includes [`DieselError::NotFound`] for an unknown category.
    #[error(transparent)]
    Database(#[from] DieselError),
    /// Moving a removed category's learnings failed.
    #[error(transparent)]
    Learning(#[from] LearningError),
    #[error("a category already has this key")]
    KeyTaken,
    #[error("Elysium's built-in categories cannot be changed or removed")]
    BuiltIn,
    /// Learnings are filed under the category, and no category to move them to was named.
    #[error("learnings are filed under this category; choose a category to move them to")]
    InUse,
    /// The category to move learnings to is the one being removed, or does not exist.
    #[error("{0}")]
    InvalidMove(&'static str),
}

/// Every category, as it is offered: built-in ones first, then the workspace's own, each in
/// its order.
///
/// # Errors
/// Propagates any database error.
pub async fn list(connection: &mut AsyncPgConnection) -> QueryResult<Vec<LearningCategory>> {
    learning_categories::table
        .order((
            learning_categories::built_in.desc(),
            learning_categories::position.asc(),
            learning_categories::key.asc(),
        ))
        .select(LearningCategory::as_select())
        .load(connection)
        .await
}

/// How many learnings, deleted ones included, each category has filed under it. A category
/// with none is left out.
///
/// Deleted learnings count because they keep their category, and are moved with the live ones
/// when it is removed.
///
/// # Errors
/// Propagates any database error.
pub async fn learning_counts(
    connection: &mut AsyncPgConnection,
) -> QueryResult<HashMap<String, i64>> {
    let counts: Vec<(String, i64)> = learnings::table
        .group_by(learnings::category)
        .select((learnings::category, diesel::dsl::count_star()))
        .load(connection)
        .await?;
    Ok(counts.into_iter().collect())
}

/// Category `key`.
///
/// # Errors
/// `NotFound` for an unknown key.
pub async fn find(connection: &mut AsyncPgConnection, key: &str) -> QueryResult<LearningCategory> {
    learning_categories::table
        .find(key)
        .select(LearningCategory::as_select())
        .first(connection)
        .await
}

/// Whether category `key` exists, holding it until the transaction ends so it cannot be
/// removed while a learning is being filed under it.
///
/// `FOR KEY SHARE` is the lock the foreign key would take, so filings never wait on each other,
/// only on a removal, which locks the row `FOR UPDATE`.
///
/// # Errors
/// Propagates any database error.
pub async fn hold(connection: &mut AsyncPgConnection, key: &str) -> QueryResult<bool> {
    learning_categories::table
        .find(key)
        .select(learning_categories::key)
        .for_key_share()
        .first::<String>(connection)
        .await
        .optional()
        .map(|found| found.is_some())
}

#[derive(Insertable)]
#[diesel(table_name = learning_categories)]
struct CategoryRow {
    key: String,
    label: String,
    description: String,
    position: i32,
    created_by: Uuid,
}

/// Adds `category` to the workspace's own, after the last of them, by `actor_id`.
///
/// # Errors
/// [`CategoryError::KeyTaken`] when a category, built-in or not, already has its key.
pub async fn create(
    connection: &mut AsyncPgConnection,
    category: NewCategory,
    actor_id: Uuid,
) -> Result<LearningCategory, CategoryError> {
    connection
        .transaction(async move |connection| {
            let last: Option<i32> = learning_categories::table
                .filter(learning_categories::built_in.eq(false))
                .select(diesel::dsl::max(learning_categories::position))
                .get_result(connection)
                .await?;
            let created = diesel::insert_into(learning_categories::table)
                .values(CategoryRow {
                    key: category.key,
                    label: category.label,
                    description: category.description,
                    position: last.map_or(0, |position| position + 1),
                    created_by: actor_id,
                })
                .returning(LearningCategory::as_returning())
                .get_result(connection)
                .await
                .map_err(|error| match error {
                    DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
                        CategoryError::KeyTaken
                    }
                    other => other.into(),
                })?;
            user::record_event(
                connection,
                None,
                actor_id,
                UserEventKind::LearningCategoryAdded,
                json!({ "category": created.recorded() }),
            )
            .await?;
            Ok(created)
        })
        .await
}

/// Applies `changes` to workspace category `key`, by `actor_id`, recording the values before
/// and after. A change that changes nothing records nothing.
///
/// # Errors
/// `NotFound` for an unknown key, and [`CategoryError::BuiltIn`] for a built-in category.
pub async fn update(
    connection: &mut AsyncPgConnection,
    key: &str,
    changes: CategoryChanges,
    actor_id: Uuid,
) -> Result<LearningCategory, CategoryError> {
    connection
        .transaction(async move |connection| {
            let before = lock(connection, key).await?;
            if before.built_in {
                return Err(CategoryError::BuiltIn);
            }

            let label = changes.label.unwrap_or_else(|| before.label.clone());
            let description = changes
                .description
                .unwrap_or_else(|| before.description.clone());
            if label != before.label || description != before.description {
                diesel::update(learning_categories::table.find(key))
                    .set((
                        learning_categories::label.eq(label),
                        learning_categories::description.eq(description),
                    ))
                    .execute(connection)
                    .await?;
            }
            if let Some(position) = changes.position {
                reorder(connection, key, position).await?;
            }

            let after = find(connection, key).await?;
            let (recorded_before, recorded_after) = (before.recorded(), after.recorded());
            if recorded_before != recorded_after {
                user::record_event(
                    connection,
                    None,
                    actor_id,
                    UserEventKind::LearningCategoryChanged,
                    json!({ "before": recorded_before, "after": recorded_after }),
                )
                .await?;
            }
            Ok(after)
        })
        .await
}

/// Moves workspace category `key` to `position` among the workspace's own categories, and
/// numbers them all again from 0 in their new order.
async fn reorder(
    connection: &mut AsyncPgConnection,
    key: &str,
    position: usize,
) -> QueryResult<()> {
    // Every workspace category is locked, in one order, so two reorders queue rather than
    // interleave.
    let mut order: Vec<(String, i32)> = learning_categories::table
        .filter(learning_categories::built_in.eq(false))
        .order((
            learning_categories::position.asc(),
            learning_categories::key.asc(),
        ))
        .select((learning_categories::key, learning_categories::position))
        .for_update()
        .load(connection)
        .await?;
    let Some(current) = order.iter().position(|(candidate, _)| candidate == key) else {
        unreachable!("the category was locked as one of the workspace's own");
    };
    let moved = order.remove(current);
    order.insert(position.min(order.len()), moved);

    for (index, (candidate, stored)) in order.into_iter().enumerate() {
        let index = i32::try_from(index).expect("a workspace has fewer than 2^31 categories");
        if stored != index {
            diesel::update(learning_categories::table.find(candidate))
                .set(learning_categories::position.eq(index))
                .execute(connection)
                .await?;
        }
    }
    Ok(())
}

/// Removes workspace category `key`, by `author`. Every learning filed under it, deleted ones
/// included, is first moved to `move_to`, each move a revision in its history; with learnings to
/// move and no `move_to`, nothing is removed.
///
/// Answers the learnings it moved, as the moves left them, so each can be announced.
///
/// # Errors
/// `NotFound` for an unknown key, [`CategoryError::BuiltIn`] for a built-in category,
/// [`CategoryError::InUse`] when learnings are filed under it and `move_to` is `None`, and
/// [`CategoryError::InvalidMove`] when `move_to` is `key` itself or no category.
pub async fn remove(
    connection: &mut AsyncPgConnection,
    key: &str,
    move_to: Option<&str>,
    author: Author<'_>,
) -> Result<Vec<Recorded>, CategoryError> {
    connection
        .transaction(async move |connection| {
            // Locking the row FOR UPDATE makes every save filing a learning under it wait, so
            // none lands between the move and the removal.
            let category = lock(connection, key).await?;
            if category.built_in {
                return Err(CategoryError::BuiltIn);
            }

            let filed: i64 = learnings::table
                .filter(learnings::category.eq(key))
                .count()
                .get_result(connection)
                .await?;
            let mut moved = Vec::new();
            if filed > 0 {
                let Some(move_to) = move_to else {
                    return Err(CategoryError::InUse);
                };
                if move_to == key {
                    return Err(CategoryError::InvalidMove(
                        "learnings cannot be moved to the category being removed",
                    ));
                }
                if !hold(connection, move_to).await? {
                    return Err(CategoryError::InvalidMove(
                        "no category has the key to move its learnings to",
                    ));
                }
                moved = learning::refile(connection, key, move_to, author).await?;
            }

            diesel::delete(learning_categories::table.find(key))
                .execute(connection)
                .await?;
            user::record_event(
                connection,
                None,
                author.user_id,
                UserEventKind::LearningCategoryRemoved,
                json!({
                    "category": category.recorded(),
                    // Where its learnings went, when it had any.
                    "movedTo": (!moved.is_empty()).then_some(move_to).flatten(),
                    "movedLearnings": moved.len(),
                }),
            )
            .await?;
            Ok(moved)
        })
        .await
}

async fn lock(connection: &mut AsyncPgConnection, key: &str) -> QueryResult<LearningCategory> {
    learning_categories::table
        .find(key)
        .for_update()
        .select(LearningCategory::as_select())
        .first(connection)
        .await
}
