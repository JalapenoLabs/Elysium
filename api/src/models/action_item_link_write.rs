// Copyright © 2026 Jalapeno Labs

//! Provider writes Elysium owes and has not yet landed: closing a linked issue when its
//! item resolves, and posting an item's comment to its primary link.
//!
//! A write is owed in the same transaction as the change that owes it, so a crash between
//! the change and the call never loses it. The watcher lands every owed write on each pass
//! (`crate::action_items::watcher`); one that fails keeps the provider's last answer here,
//! which is what the item page shows as the link's pending state, until it lands or the
//! user cancels it. See `docs/action-items.md`.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::action_items::{Actor, WorkError};
use crate::database::schema::{action_item_link_writes, action_item_links};
use crate::models::action_item::{self, ActionItem};
use crate::models::action_item_event::{HistoryKind, Recorded};
use crate::models::action_item_link::{self, LinkKind, LinkState};

/// What a write does.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::LinkWriteKind"]
#[serde(rename_all = "kebab-case")]
pub enum LinkWriteKind {
    /// Move the linked issue to done: Jira's done transition, or GitHub's close as completed.
    Close,
    /// Post a comment to the link.
    Comment,
}

impl LinkWriteKind {
    /// The name history records.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Close => "close",
            Self::Comment => "comment",
        }
    }
}

/// The longest provider answer a write keeps, matching its column.
const ERROR_MAX_CHARACTERS: usize = 2000;

/// A write that has not landed.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = action_item_link_writes, check_for_backend(diesel::pg::Pg))]
pub struct LinkWrite {
    pub id: Uuid,
    pub link_id: Uuid,
    pub kind: LinkWriteKind,
    /// Set exactly for a comment.
    pub comment_id: Option<Uuid>,
    /// How many times it was tried.
    pub attempts: i32,
    /// The provider's last answer, or why the write waits.
    pub last_error: Option<String>,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = action_item_link_writes)]
struct WriteRow {
    id: Uuid,
    link_id: Uuid,
    kind: LinkWriteKind,
    comment_id: Option<Uuid>,
    created_at: DateTime<Utc>,
}

/// Owes a close for every issue linked to the item that is still open, as last read. Pull
/// requests are never closed. A link that already owes one owes it once.
///
/// Call it inside the transaction that resolves the item. Returns the links that now owe a
/// close, including ones that already did.
///
/// # Errors
/// Propagates any database error.
pub async fn owe_closes(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
    now: DateTime<Utc>,
) -> QueryResult<Vec<Uuid>> {
    let link_ids: Vec<Uuid> = action_item_links::table
        .filter(action_item_links::action_item_id.eq(action_item_id))
        .filter(action_item_links::kind.eq(LinkKind::Issue))
        .filter(action_item_links::observed_state.eq(LinkState::Open))
        .order(action_item_links::id)
        .select(action_item_links::id)
        .load(connection)
        .await?;
    let rows: Vec<WriteRow> = link_ids
        .iter()
        .map(|&link_id| WriteRow {
            id: Uuid::now_v7(),
            link_id,
            kind: LinkWriteKind::Close,
            comment_id: None,
            created_at: now,
        })
        .collect();
    diesel::insert_into(action_item_link_writes::table)
        .values(rows)
        .on_conflict_do_nothing()
        .execute(connection)
        .await?;
    Ok(link_ids)
}

/// Owes a comment to the item's primary link, when it has one. Returns that link.
///
/// Call it inside the transaction that writes the comment.
///
/// # Errors
/// Propagates any database error.
pub async fn owe_comment(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
    comment_id: Uuid,
    now: DateTime<Utc>,
) -> QueryResult<Option<Uuid>> {
    let primary: Option<Uuid> = action_item_links::table
        .filter(action_item_links::action_item_id.eq(action_item_id))
        .filter(action_item_links::is_primary)
        .select(action_item_links::id)
        .first(connection)
        .await
        .optional()?;
    let Some(link_id) = primary else {
        return Ok(None);
    };
    diesel::insert_into(action_item_link_writes::table)
        .values(WriteRow {
            id: Uuid::now_v7(),
            link_id,
            kind: LinkWriteKind::Comment,
            comment_id: Some(comment_id),
            created_at: now,
        })
        .execute(connection)
        .await?;
    Ok(Some(link_id))
}

/// The writes `link_ids` owe, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn for_links(
    connection: &mut AsyncPgConnection,
    link_ids: &[Uuid],
) -> QueryResult<Vec<LinkWrite>> {
    action_item_link_writes::table
        .filter(action_item_link_writes::link_id.eq_any(link_ids))
        .order(action_item_link_writes::id)
        .select(LinkWrite::as_select())
        .load(connection)
        .await
}

/// Up to `limit` owed writes: never-tried ones first, then the ones tried longest ago.
///
/// # Errors
/// Propagates any database error.
pub async fn owed(connection: &mut AsyncPgConnection, limit: i64) -> QueryResult<Vec<LinkWrite>> {
    // Untried writes first, then the ones tried longest ago, so writes that keep failing or
    // waiting take turns behind new ones instead of holding the front of the queue.
    action_item_link_writes::table
        .order((
            action_item_link_writes::last_attempt_at.asc().nulls_first(),
            action_item_link_writes::id,
        ))
        .limit(limit)
        .select(LinkWrite::as_select())
        .load(connection)
        .await
}

/// Records a failed attempt and the provider's answer, or why the write waits.
///
/// # Errors
/// Propagates any database error.
pub async fn record_attempt(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    message: &str,
    now: DateTime<Utc>,
) -> QueryResult<()> {
    let message: String = message.chars().take(ERROR_MAX_CHARACTERS).collect();
    diesel::update(action_item_link_writes::table.find(id))
        .set((
            action_item_link_writes::attempts.eq(action_item_link_writes::attempts + 1),
            action_item_link_writes::last_error.eq(message),
            action_item_link_writes::last_attempt_at.eq(now),
        ))
        .execute(connection)
        .await?;
    Ok(())
}

/// Forgets a write that landed, or that no longer applies.
///
/// # Errors
/// Propagates any database error.
pub async fn complete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    diesel::delete(action_item_link_writes::table.find(id))
        .execute(connection)
        .await?;
    Ok(())
}

/// The user gives up on a write that has not landed, which the item's history records.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when the item has no such link or the link
/// owes no such write, [`WorkError::Conflict`] for a deleted item, and any other database
/// error.
pub async fn cancel(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
    link_id: Uuid,
    write_id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    connection
        .transaction(async move |connection| {
            let item = action_item::lock_live(connection, action_item_id).await?;
            let link = action_item_link::find_for_item(connection, action_item_id, link_id).await?;
            let write: LinkWrite = action_item_link_writes::table
                .filter(action_item_link_writes::id.eq(write_id))
                .filter(action_item_link_writes::link_id.eq(link_id))
                .select(LinkWrite::as_select())
                .first(connection)
                .await?;
            complete(connection, write.id).await?;
            let entry = action_item_link::record_on_item(
                connection,
                action_item_id,
                HistoryKind::LinkWriteCancelled,
                actor,
                json!({
                    "linkId": link.id,
                    "key": link.external_key,
                    "write": write.kind.as_str(),
                }),
                now,
            )
            .await?;
            Ok(Recorded {
                record: item,
                history: vec![entry],
            })
        })
        .await
}
