// Copyright © 2026 Jalapeno Labs

//! Two meetings that look like one, and what their person decided. See `crate::meetings::duplicates`
//! and `docs/meetings.md`, The same meeting twice.
//!
//! A pair is recorded once, whichever meeting found the other (`meeting_duplicates_once_per_pair`):
//! the meeting asked about, the one it looks like, the inbox item asking, and, once its person
//! decides, whether the two were merged or are not the same.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::meeting_duplicates;

/// What a meeting's person decided about a pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// The meeting's captures were merged into the other.
    Merged,
    /// The two are different meetings.
    NotTheSame,
}

impl Decision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Merged => "merged",
            Self::NotTheSame => "not_the_same",
        }
    }
}

/// Two meetings that look like one.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = meeting_duplicates, check_for_backend(diesel::pg::Pg))]
pub struct MeetingDuplicate {
    pub id: Uuid,
    /// The meeting asked about, whose captures a merge moves.
    pub meeting_id: Uuid,
    /// The meeting it looks like, which a merge moves them into.
    pub same_as_id: Uuid,
    /// The inbox item asking, while it exists.
    pub action_item_id: Option<Uuid>,
    /// A [`Decision`] name, or `None` until its person decides.
    pub decision: Option<String>,
    pub decided_by: Option<Uuid>,
    pub decided_at: Option<DateTime<Utc>>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A pair about to be recorded.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = meeting_duplicates)]
pub struct NewMeetingDuplicate {
    pub id: Uuid,
    pub meeting_id: Uuid,
    pub same_as_id: Uuid,
    pub action_item_id: Option<Uuid>,
    pub created_by: Uuid,
}

/// Records `pair`, answering `None`, writing nothing, when the two meetings were recorded as a
/// pair already, either way round.
///
/// # Errors
/// Propagates any database error.
pub async fn insert_new(
    connection: &mut AsyncPgConnection,
    pair: NewMeetingDuplicate,
) -> QueryResult<Option<MeetingDuplicate>> {
    // The unique index is on the pair in either order, which only an untargeted conflict names.
    diesel::insert_into(meeting_duplicates::table)
        .values(pair)
        .on_conflict_do_nothing()
        .returning(MeetingDuplicate::as_returning())
        .get_result(connection)
        .await
        .optional()
}

/// Records `action_item_id` as the inbox item asking about pair `id`.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when there is no such pair.
pub async fn set_item(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    action_item_id: Uuid,
) -> QueryResult<MeetingDuplicate> {
    diesel::update(meeting_duplicates::table.find(id))
        .set(meeting_duplicates::action_item_id.eq(Some(action_item_id)))
        .returning(MeetingDuplicate::as_returning())
        .get_result(connection)
        .await
}

/// Pair `id`.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when there is none.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<MeetingDuplicate> {
    meeting_duplicates::table
        .find(id)
        .select(MeetingDuplicate::as_select())
        .first(connection)
        .await
}

/// Every pair meeting `meeting_id` is in, either way round, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn of_meeting(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> QueryResult<Vec<MeetingDuplicate>> {
    meeting_duplicates::table
        .filter(
            meeting_duplicates::meeting_id
                .eq(meeting_id)
                .or(meeting_duplicates::same_as_id.eq(meeting_id)),
        )
        .order(meeting_duplicates::created_at.asc())
        .select(MeetingDuplicate::as_select())
        .load(connection)
        .await
}

/// The pair inbox item `action_item_id` asks about, if it asks about one.
///
/// # Errors
/// Propagates any database error.
pub async fn asked_by(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
) -> QueryResult<Option<MeetingDuplicate>> {
    meeting_duplicates::table
        .filter(meeting_duplicates::action_item_id.eq(action_item_id))
        .select(MeetingDuplicate::as_select())
        .first(connection)
        .await
        .optional()
}

/// Records `decision` on pair `id`, by `decided_by` at `decided_at`, whatever was decided before:
/// a split makes a merged pair not the same, and its undo merges it again.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when there is no such pair.
pub async fn decide(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    decision: Decision,
    (decided_by, decided_at): (Uuid, DateTime<Utc>),
) -> QueryResult<MeetingDuplicate> {
    diesel::update(meeting_duplicates::table.find(id))
        .set((
            meeting_duplicates::decision.eq(Some(decision.as_str())),
            meeting_duplicates::decided_by.eq(Some(decided_by)),
            meeting_duplicates::decided_at.eq(Some(decided_at)),
        ))
        .returning(MeetingDuplicate::as_returning())
        .get_result(connection)
        .await
}
