// Copyright © 2026 Jalapeno Labs

//! What the engine's worker pool knows that the work queue's page shows: the CPU cores the engine
//! sees, which the pool's default size follows, and how long every Claude account's usage holds
//! slots back. One row, written by the engine as it looks. See `crate::worker_pool`.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::upsert::excluded;
use diesel_async::{AsyncPgConnection, RunQueryDsl};

use crate::database::schema::worker_pool;
use crate::models::user::SYSTEM_USER_ID;

/// The pool as the engine last saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = worker_pool, check_for_backend(diesel::pg::Pg))]
pub struct WorkerPool {
    /// The CPU cores the engine sees.
    pub cpu_count: i32,
    /// Set while every Claude credential that could serve is at its usage limit.
    pub usage_held_until: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

/// The pool as the engine last saw it; `None` before an engine has looked.
///
/// # Errors
/// Propagates any database error.
pub async fn find(connection: &mut AsyncPgConnection) -> QueryResult<Option<WorkerPool>> {
    worker_pool::table
        .select(WorkerPool::as_select())
        .first(connection)
        .await
        .optional()
}

/// Records what the engine sees: `cpu_count` cores, and slots held back until
/// `usage_held_until`. Writes only what changed, and answers whether anything did.
///
/// # Errors
/// Propagates any database error, including a count below one.
pub async fn record(
    connection: &mut AsyncPgConnection,
    cpu_count: i32,
    usage_held_until: Option<DateTime<Utc>>,
) -> QueryResult<bool> {
    // One engine writes the row, so nothing changes it between the read and the write.
    let unchanged = find(connection).await?.is_some_and(|seen| {
        seen.cpu_count == cpu_count && seen.usage_held_until == usage_held_until
    });
    if unchanged {
        return Ok(false);
    }
    diesel::insert_into(worker_pool::table)
        .values((
            worker_pool::cpu_count.eq(cpu_count),
            worker_pool::usage_held_until.eq(usage_held_until),
            worker_pool::created_by.eq(SYSTEM_USER_ID),
        ))
        .on_conflict(worker_pool::singleton)
        .do_update()
        .set((
            worker_pool::cpu_count.eq(excluded(worker_pool::cpu_count)),
            worker_pool::usage_held_until.eq(excluded(worker_pool::usage_held_until)),
        ))
        .execute(connection)
        .await?;
    Ok(true)
}
