// Copyright © 2026 Jalapeno Labs

//! Comments written on an action item.
//!
//! Only a comment's author edits or deletes it: the user cannot rewrite what Elysia or an
//! agent said, and the reverse. Every write is recorded in the item's history with enough
//! to put the comment back, and a deleted item takes no comment writes until restored.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde_json::json;
use uuid::Uuid;

use crate::action_items::{Actor, WorkError};
use crate::database::schema::action_item_comments;
use crate::models::action_item_event::{self, Change, HistoryKind, Recorded, Subject};
use crate::models::{action_item, action_item_link_write};

/// A stored comment.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = action_item_comments, check_for_backend(diesel::pg::Pg))]
pub struct Comment {
    pub id: Uuid,
    pub action_item_id: Uuid,
    /// An [`Actor`] in its recorded form.
    pub author: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = action_item_comments)]
struct CommentRow {
    id: Uuid,
    action_item_id: Uuid,
    author: String,
    body: String,
    created_at: DateTime<Utc>,
    /// Written with `created_at` rather than left to the column default, so a comment that
    /// was never edited has equal timestamps and clients can tell an edited one apart.
    updated_at: DateTime<Utc>,
}

/// Loads one of the item's comments for a write by `actor`, locking its row.
async fn lock_own(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
    comment_id: Uuid,
    actor: Actor,
) -> Result<Comment, WorkError> {
    let comment: Comment = action_item_comments::table
        .find(comment_id)
        .filter(action_item_comments::action_item_id.eq(action_item_id))
        .for_update()
        .select(Comment::as_select())
        .first(connection)
        .await?;
    if comment.author != actor.to_string() {
        return Err(WorkError::Conflict("only a comment's author can change it"));
    }
    Ok(comment)
}

/// The item's comments, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
) -> QueryResult<Vec<Comment>> {
    action_item_comments::table
        .filter(action_item_comments::action_item_id.eq(action_item_id))
        .order((
            action_item_comments::created_at.asc(),
            action_item_comments::id.asc(),
        ))
        .select(Comment::as_select())
        .load(connection)
        .await
}

/// One comment by id.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no comment has that id.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Comment> {
    action_item_comments::table
        .find(id)
        .select(Comment::as_select())
        .first(connection)
        .await
}

/// Writes a comment on a live item, and owes it to the item's primary link, if it has one,
/// in the same transaction: a comment written on an item is posted to its primary link.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown item, [`WorkError::Conflict`]
/// for a deleted one, and any other database error.
pub async fn create(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
    body: String,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<Comment>, WorkError> {
    connection
        .transaction(async move |connection| {
            action_item::lock_live(connection, action_item_id).await?;
            let record: Comment = diesel::insert_into(action_item_comments::table)
                .values(CommentRow {
                    id: Uuid::now_v7(),
                    action_item_id,
                    author: actor.to_string(),
                    body,
                    created_at: now,
                    updated_at: now,
                })
                .returning(Comment::as_returning())
                .get_result(connection)
                .await?;
            action_item_link_write::owe_comment(connection, action_item_id, record.id, now).await?;
            let entry = action_item_event::record(
                connection,
                Change {
                    subject: Subject::Item(action_item_id),
                    kind: HistoryKind::Commented,
                    actor,
                    data: json!({ "commentId": record.id, "body": record.body }),
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

/// Replaces the body of one of `actor`'s comments on a live item.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown item or a comment that is
/// not on it, [`WorkError::Conflict`] for a deleted item or someone else's comment, and
/// any other database error.
pub async fn update(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
    comment_id: Uuid,
    body: String,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<Comment>, WorkError> {
    connection
        .transaction(async move |connection| {
            action_item::lock_live(connection, action_item_id).await?;
            let comment = lock_own(connection, action_item_id, comment_id, actor).await?;
            if comment.body == body {
                return Ok(Recorded {
                    record: comment,
                    history: Vec::new(),
                });
            }
            let record = diesel::update(action_item_comments::table.find(comment_id))
                .set(action_item_comments::body.eq(&body))
                .returning(Comment::as_returning())
                .get_result(connection)
                .await?;
            let entry = action_item_event::record(
                connection,
                Change {
                    subject: Subject::Item(action_item_id),
                    kind: HistoryKind::CommentEdited,
                    actor,
                    data: json!({ "commentId": comment_id, "from": comment.body, "to": body }),
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

/// Deletes one of `actor`'s comments on a live item, keeping its body in the history.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown item or a comment that is
/// not on it, [`WorkError::Conflict`] for a deleted item or someone else's comment, and
/// any other database error.
pub async fn delete(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
    comment_id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<Comment>, WorkError> {
    connection
        .transaction(async move |connection| {
            action_item::lock_live(connection, action_item_id).await?;
            let record = lock_own(connection, action_item_id, comment_id, actor).await?;
            diesel::delete(action_item_comments::table.find(comment_id))
                .execute(connection)
                .await?;
            let entry = action_item_event::record(
                connection,
                Change {
                    subject: Subject::Item(action_item_id),
                    kind: HistoryKind::CommentDeleted,
                    actor,
                    data: json!({ "commentId": comment_id, "body": record.body }),
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

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::models::action_item::{ActionItemPriority, ActionItemState, NewActionItem, Owner};
    use crate::models::action_item_event::list_for_item;
    use crate::test_support::migrated_database;

    fn minute(offset: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z")
            .expect("a valid instant")
            .with_timezone(&Utc)
            + TimeDelta::minutes(offset)
    }

    async fn create_item(connection: &mut AsyncPgConnection) -> Uuid {
        let new_item = NewActionItem {
            title: "Review the storage PR".to_owned(),
            notes: String::new(),
            state: ActionItemState::Open,
            priority: ActionItemPriority::Normal,
            due_at: None,
            owner: Owner::User,
            project_ids: Vec::new(),
            initiative_ids: Vec::new(),
        };
        action_item::create(connection, new_item, Actor::User, minute(0))
            .await
            .expect("item")
            .record
            .id
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn comments_are_written_edited_and_deleted_with_their_history() {
        let (_url, mut connection) = migrated_database().await;
        let item = create_item(&mut connection).await;

        let first = create(
            &mut connection,
            item,
            "Looks close".to_owned(),
            Actor::User,
            minute(1),
        )
        .await
        .expect("comment")
        .record;
        create(
            &mut connection,
            item,
            "Asked for tests".to_owned(),
            Actor::User,
            minute(2),
        )
        .await
        .expect("comment");
        assert_eq!(first.author, "user");
        assert_eq!(
            first.updated_at, first.created_at,
            "a comment never edited was last updated when it was written"
        );
        let bodies: Vec<String> = list(&mut connection, item)
            .await
            .expect("list")
            .into_iter()
            .map(|comment| comment.body)
            .collect();
        assert_eq!(bodies, ["Looks close", "Asked for tests"]);

        let same = update(
            &mut connection,
            item,
            first.id,
            "Looks close".to_owned(),
            Actor::User,
            minute(3),
        )
        .await
        .expect("edit");
        assert!(same.history.is_empty(), "an unchanged body records nothing");
        let edited = update(
            &mut connection,
            item,
            first.id,
            "Looks good".to_owned(),
            Actor::User,
            minute(3),
        )
        .await
        .expect("edit");
        assert_eq!(edited.record.body, "Looks good");
        assert!(
            edited.record.updated_at > edited.record.created_at,
            "an edit moves updated_at"
        );
        assert_eq!(
            edited.history[0].data,
            json!({ "commentId": first.id, "from": "Looks close", "to": "Looks good" })
        );

        delete(&mut connection, item, first.id, Actor::User, minute(4))
            .await
            .expect("delete");
        assert_eq!(list(&mut connection, item).await.expect("list").len(), 1);

        let history = list_for_item(&mut connection, item).await.expect("history");
        let kinds: Vec<&str> = history.iter().map(|entry| entry.kind.as_str()).collect();
        assert_eq!(
            kinds,
            [
                "created",
                "commented",
                "commented",
                "comment_edited",
                "comment_deleted"
            ]
        );
        assert_eq!(
            history[4].data,
            json!({ "commentId": first.id, "body": "Looks good" }),
            "a deleted comment's body stays in the history"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn only_the_author_changes_a_comment_and_deleted_items_take_none() {
        let (_url, mut connection) = migrated_database().await;
        let item = create_item(&mut connection).await;
        let other_item = create_item(&mut connection).await;

        // Elysia's comments arrive in a later stage; write one directly to stand in.
        let elysia_comment = Uuid::now_v7();
        diesel::insert_into(action_item_comments::table)
            .values(CommentRow {
                id: elysia_comment,
                action_item_id: item,
                author: "elysia".to_owned(),
                body: "The PR fixes #12.".to_owned(),
                created_at: minute(1),
                updated_at: minute(1),
            })
            .execute(&mut connection)
            .await
            .expect("insert");
        let rewrite = update(
            &mut connection,
            item,
            elysia_comment,
            "No".to_owned(),
            Actor::User,
            minute(2),
        )
        .await;
        assert!(matches!(rewrite, Err(WorkError::Conflict(_))));
        let removal = delete(
            &mut connection,
            item,
            elysia_comment,
            Actor::User,
            minute(2),
        )
        .await;
        assert!(matches!(removal, Err(WorkError::Conflict(_))));

        let wrong_item = delete(
            &mut connection,
            other_item,
            elysia_comment,
            Actor::User,
            minute(2),
        )
        .await;
        assert!(matches!(
            wrong_item,
            Err(WorkError::Database(diesel::result::Error::NotFound))
        ));

        action_item::soft_delete(&mut connection, item, Actor::User, minute(3))
            .await
            .expect("delete item");
        let on_deleted = create(
            &mut connection,
            item,
            "Hello?".to_owned(),
            Actor::User,
            minute(4),
        )
        .await;
        assert!(matches!(on_deleted, Err(WorkError::Conflict(_))));

        let bad_author = diesel::insert_into(action_item_comments::table)
            .values(CommentRow {
                id: Uuid::now_v7(),
                action_item_id: other_item,
                author: "session:0".to_owned(),
                body: "x".to_owned(),
                created_at: minute(5),
                updated_at: minute(5),
            })
            .execute(&mut connection)
            .await;
        bad_author.expect_err("action_item_comments_author_shape refuses session:0");
    }
}
