// Copyright © 2026 Jalapeno Labs

//! Studio items: one asset each, and the sessions that make it.
//!
//! An item holds what Studio adds around its sessions: a title, the first prompt, an optional
//! project, the storage location its files are kept in, and a pinned thumbnail. Deleting is
//! soft by default; a permanent delete is the route's to orchestrate, because the item's
//! files must leave the storage location before its rows go. See `docs/studio.md`.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::studio_items;

/// A stored item, live or softly deleted.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = studio_items, check_for_backend(diesel::pg::Pg))]
pub struct StudioItem {
    pub id: Uuid,
    pub title: String,
    /// The first prompt, which a continued item's brief repeats.
    pub prompt: String,
    pub project_id: Option<Uuid>,
    pub storage_location_id: Uuid,
    /// The pinned thumbnail; `None` for the default (`crate::studio::thumbnail`).
    pub thumbnail_asset_id: Option<Uuid>,
    /// Why the latest file could not be kept; cleared by the next file that is.
    pub pull_error: Option<String>,
    pub deleted_at: Option<DateTime<Utc>>,
    /// Who created it.
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Fields for a new item.
#[derive(Debug, Insertable)]
#[diesel(table_name = studio_items)]
pub struct NewStudioItem {
    /// Who is creating it.
    pub created_by: Uuid,
    pub id: Uuid,
    pub title: String,
    pub prompt: String,
    pub project_id: Option<Uuid>,
    pub storage_location_id: Uuid,
}

/// Which items a listing returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    Live,
    Deleted,
}

/// Items newest first, live or softly deleted.
///
/// # Errors
/// Propagates any database error.
pub async fn list(
    connection: &mut AsyncPgConnection,
    liveness: Liveness,
) -> QueryResult<Vec<StudioItem>> {
    let query = studio_items::table
        .order(studio_items::created_at.desc())
        .select(StudioItem::as_select())
        .into_boxed();
    let query = match liveness {
        Liveness::Live => query.filter(studio_items::deleted_at.is_null()),
        Liveness::Deleted => query.filter(studio_items::deleted_at.is_not_null()),
    };
    query.load(connection).await
}

/// One item by id, live or softly deleted.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<StudioItem> {
    studio_items::table
        .find(id)
        .select(StudioItem::as_select())
        .first(connection)
        .await
}

/// Records an item.
///
/// # Errors
/// Propagates database errors, including a foreign key violation for an unknown project or
/// storage location.
pub async fn create(
    connection: &mut AsyncPgConnection,
    new_item: &NewStudioItem,
) -> QueryResult<StudioItem> {
    diesel::insert_into(studio_items::table)
        .values(new_item)
        .returning(StudioItem::as_returning())
        .get_result(connection)
        .await
}

/// Renames an item.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn rename(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    title: &str,
) -> QueryResult<StudioItem> {
    diesel::update(studio_items::table.find(id))
        .set(studio_items::title.eq(title))
        .returning(StudioItem::as_returning())
        .get_result(connection)
        .await
}

/// Pins an image as the item's thumbnail, or unpins with `None`. The caller checks that the
/// image is one of the item's.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn pin_thumbnail(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    asset_id: Option<Uuid>,
) -> QueryResult<StudioItem> {
    diesel::update(studio_items::table.find(id))
        .set(studio_items::thumbnail_asset_id.eq(asset_id))
        .returning(StudioItem::as_returning())
        .get_result(connection)
        .await
}

/// Records why a file could not be kept, or clears it with `None` once one is.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn set_pull_error(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    pull_error: Option<&str>,
) -> QueryResult<StudioItem> {
    diesel::update(studio_items::table.find(id))
        .set(studio_items::pull_error.eq(pull_error))
        .returning(StudioItem::as_returning())
        .get_result(connection)
        .await
}

/// Hides an item until it is restored, or restores it with `None`.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn set_deleted_at(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    deleted_at: Option<DateTime<Utc>>,
) -> QueryResult<StudioItem> {
    diesel::update(studio_items::table.find(id))
        .set(studio_items::deleted_at.eq(deleted_at))
        .returning(StudioItem::as_returning())
        .get_result(connection)
        .await
}

/// Removes an item for good, with its assets, feedback, and sessions (and so their events
/// and transcripts), which all cascade from it. Its files must already be gone from its
/// storage location.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn delete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    let deleted = diesel::delete(studio_items::table.find(id))
        .execute(connection)
        .await?;
    if deleted == 0 {
        return Err(diesel::result::Error::NotFound);
    }
    Ok(())
}

/// Bytes Studio keeps in a storage location, counting every item's files and feedback
/// images, which is what the location's storage limit is checked against.
///
/// Files are named by content, so a file two versions share is stored once but counted
/// twice. That can only overstate usage, which errs toward the limit rather than past it.
///
/// # Errors
/// Propagates any database error.
pub async fn bytes_in_location(
    connection: &mut AsyncPgConnection,
    storage_location_id: Uuid,
) -> QueryResult<i64> {
    #[derive(QueryableByName)]
    struct Usage {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        bytes: i64,
    }

    // `SUM(BIGINT)` is `NUMERIC`; the sums are cast back once, in SQL, where it reads plainly.
    let usage: Usage = diesel::sql_query(
        "SELECT \
            COALESCE(( \
                SELECT SUM(asset.size_bytes) \
                FROM studio_assets asset \
                JOIN studio_items item ON item.id = asset.studio_item_id \
                WHERE item.storage_location_id = $1 \
            ), 0)::BIGINT \
          + COALESCE(( \
                SELECT SUM(feedback.annotated_size_bytes + COALESCE(feedback.capture_size_bytes, 0)) \
                FROM studio_feedback feedback \
                JOIN studio_items item ON item.id = feedback.studio_item_id \
                WHERE item.storage_location_id = $1 \
            ), 0)::BIGINT \
          AS bytes",
    )
    .bind::<diesel::sql_types::Uuid, _>(storage_location_id)
    .get_result(connection)
    .await?;
    Ok(usage.bytes)
}
