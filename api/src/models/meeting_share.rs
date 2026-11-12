// Copyright © 2026 Jalapeno Labs

//! Whom a meeting is shared with (`meeting_shares`). A private meeting is read by its owner and
//! these people; a guest reads only a meeting shared with them, public or not. Who may share is
//! `crate::meetings::access`'s, and who reads is `models::meeting::readable_by`'s, which reads
//! this table. See `docs/meeting-review.md`, Who sees a meeting.

use std::collections::HashMap;

use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::meeting_shares;

/// The people each of `meeting_ids` is shared with, in the order they were shared with, read in
/// one query. A meeting shared with nobody is absent.
///
/// # Errors
/// Propagates any database error.
pub async fn of_meetings(
    connection: &mut AsyncPgConnection,
    meeting_ids: &[Uuid],
) -> QueryResult<HashMap<Uuid, Vec<Uuid>>> {
    let rows: Vec<(Uuid, Uuid)> = meeting_shares::table
        .filter(meeting_shares::meeting_id.eq_any(meeting_ids))
        .select((meeting_shares::meeting_id, meeting_shares::person_id))
        .order((
            meeting_shares::created_at.asc(),
            meeting_shares::person_id.asc(),
        ))
        .load(connection)
        .await?;
    let mut shared: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for (meeting_id, person_id) in rows {
        shared.entry(meeting_id).or_default().push(person_id);
    }
    Ok(shared)
}

/// Shares meeting `meeting_id` with `person_id`, as `created_by`. Answers whether it was not
/// shared with them already.
///
/// # Errors
/// Propagates any database error, including a foreign key violation for someone who does not
/// exist.
pub async fn add(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    person_id: Uuid,
    created_by: Uuid,
) -> QueryResult<bool> {
    let inserted = diesel::insert_into(meeting_shares::table)
        .values((
            meeting_shares::meeting_id.eq(meeting_id),
            meeting_shares::person_id.eq(person_id),
            meeting_shares::created_by.eq(created_by),
        ))
        .on_conflict_do_nothing()
        .execute(connection)
        .await?;
    Ok(inserted == 1)
}

/// Stops sharing meeting `meeting_id` with `person_id`. Answers whether it was shared with them.
///
/// # Errors
/// Propagates any database error.
pub async fn remove(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    person_id: Uuid,
) -> QueryResult<bool> {
    let removed = diesel::delete(
        meeting_shares::table
            .filter(meeting_shares::meeting_id.eq(meeting_id))
            .filter(meeting_shares::person_id.eq(person_id)),
    )
    .execute(connection)
    .await?;
    Ok(removed == 1)
}
