// Copyright © 2026 Jalapeno Labs

//! Prompts sent to a Studio item with a drawing over one of its views.
//!
//! The drawing reaches the agent as a turn attachment; this row is Elysium's record of it,
//! with both images kept in the item's storage location. See `docs/studio.md`, Feedback.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::studio_feedback;

/// A drawn prompt.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Insertable)]
#[diesel(table_name = studio_feedback, check_for_backend(diesel::pg::Pg))]
pub struct StudioFeedback {
    pub id: Uuid,
    pub studio_item_id: Uuid,
    pub session_id: Option<i64>,
    /// The turn it started; `None` until the satellite accepts the turn.
    pub turn_id: Option<String>,
    pub prompt: String,
    /// What was drawn over, when it was one of the item's files.
    pub source_asset_id: Option<Uuid>,
    /// The 3D viewer's camera orbit at capture, as `model-viewer` writes it.
    pub camera_orbit: Option<String>,
    pub annotated_storage_path: String,
    pub annotated_size_bytes: i64,
    /// The view without the drawing.
    pub capture_storage_path: Option<String>,
    pub capture_size_bytes: Option<i64>,
    /// Who sent it.
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

/// Records drawn feedback whose images are already stored.
///
/// # Errors
/// Propagates any database error.
pub async fn create(
    connection: &mut AsyncPgConnection,
    feedback: &StudioFeedback,
) -> QueryResult<StudioFeedback> {
    diesel::insert_into(studio_feedback::table)
        .values(feedback)
        .returning(StudioFeedback::as_returning())
        .get_result(connection)
        .await
}

/// Records the turn a drawn prompt started, once the satellite accepted it.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn set_turn_id(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    turn_id: &str,
) -> QueryResult<StudioFeedback> {
    diesel::update(studio_feedback::table.find(id))
        .set(studio_feedback::turn_id.eq(Some(turn_id)))
        .returning(StudioFeedback::as_returning())
        .get_result(connection)
        .await
}

/// Forgets a drawn prompt whose turn never started.
///
/// # Errors
/// Propagates any database error.
pub async fn delete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    diesel::delete(studio_feedback::table.find(id))
        .execute(connection)
        .await?;
    Ok(())
}

/// Every drawn prompt sent to an item, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list_for_item(
    connection: &mut AsyncPgConnection,
    studio_item_id: Uuid,
) -> QueryResult<Vec<StudioFeedback>> {
    studio_feedback::table
        .filter(studio_feedback::studio_item_id.eq(studio_item_id))
        .order(studio_feedback::created_at.asc())
        .select(StudioFeedback::as_select())
        .load(connection)
        .await
}

/// One drawn prompt of an item.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when the item has none with that id.
pub async fn find_for_item(
    connection: &mut AsyncPgConnection,
    studio_item_id: Uuid,
    id: Uuid,
) -> QueryResult<StudioFeedback> {
    studio_feedback::table
        .find(id)
        .filter(studio_feedback::studio_item_id.eq(studio_item_id))
        .select(StudioFeedback::as_select())
        .first(connection)
        .await
}
