// Copyright © 2026 Jalapeno Labs

//! What auto-update has learned, in one row (`update_checks`), wherever it learned it: the
//! engine checks Docker Hub every hour, while the API answers the updater, Check now, and
//! Update now. See `crate::updates`.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::update_checks;
use crate::models::user::SYSTEM_USER_ID;

/// The row.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = update_checks, check_for_backend(diesel::pg::Pg))]
pub struct UpdateCheck {
    /// The newest release Docker Hub listed when it last answered.
    pub newest_release: Option<String>,
    pub last_checked_at: Option<DateTime<Utc>>,
    /// Why the last check failed, until one succeeds.
    pub last_check_error: Option<String>,
    /// When the updater last asked what to do.
    pub updater_seen_at: Option<DateTime<Utc>>,
    /// The release an admin asked for with Update now.
    pub requested_release: Option<String>,
    pub requested_by: Option<Uuid>,
}

/// The row, which the migration creates.
///
/// # Errors
/// Propagates any database error.
pub async fn find(connection: &mut AsyncPgConnection) -> QueryResult<UpdateCheck> {
    update_checks::table
        .select(UpdateCheck::as_select())
        .first(connection)
        .await
}

/// Records a check of Docker Hub: the newest release it listed, or why it could not be read.
/// A failed check keeps the newest release the last one found.
///
/// # Errors
/// Propagates any database error.
pub async fn record_check(
    connection: &mut AsyncPgConnection,
    checked: Result<Option<&str>, &str>,
) -> QueryResult<()> {
    let row = update_checks::table.find(true);
    let stamp = (
        update_checks::last_checked_at.eq(diesel::dsl::now),
        update_checks::updated_by.eq(SYSTEM_USER_ID),
        update_checks::updated_at.eq(diesel::dsl::now),
    );
    let updated = match checked {
        Ok(newest) => {
            diesel::update(row)
                .set((
                    stamp,
                    update_checks::newest_release.eq(newest),
                    update_checks::last_check_error.eq(None::<&str>),
                ))
                .execute(connection)
                .await
        }
        Err(error) => {
            diesel::update(row)
                .set((stamp, update_checks::last_check_error.eq(error)))
                .execute(connection)
                .await
        }
    };
    updated.map(drop)
}

/// Records that the updater asked for an instruction just now.
///
/// # Errors
/// Propagates any database error.
pub async fn record_updater_seen(connection: &mut AsyncPgConnection) -> QueryResult<()> {
    diesel::update(update_checks::table.find(true))
        .set(update_checks::updater_seen_at.eq(diesel::dsl::now))
        .execute(connection)
        .await
        .map(drop)
}

/// Records that `by` asked for `release`.
///
/// # Errors
/// Propagates any database error.
pub async fn request(
    connection: &mut AsyncPgConnection,
    release: &str,
    by: Uuid,
) -> QueryResult<()> {
    diesel::update(update_checks::table.find(true))
        .set((
            update_checks::requested_release.eq(release),
            update_checks::requested_by.eq(by),
            update_checks::updated_by.eq(by),
            update_checks::updated_at.eq(diesel::dsl::now),
        ))
        .execute(connection)
        .await
        .map(drop)
}

/// Forgets a request for `release`, once an update to it ended.
///
/// # Errors
/// Propagates any database error.
pub async fn clear_request(connection: &mut AsyncPgConnection, release: &str) -> QueryResult<()> {
    diesel::update(
        update_checks::table
            .find(true)
            .filter(update_checks::requested_release.eq(release)),
    )
    .set((
        update_checks::requested_release.eq(None::<&str>),
        update_checks::requested_by.eq(None::<Uuid>),
        update_checks::updated_by.eq(SYSTEM_USER_ID),
        update_checks::updated_at.eq(diesel::dsl::now),
    ))
    .execute(connection)
    .await
    .map(drop)
}
