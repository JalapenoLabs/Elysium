// Copyright © 2026 Jalapeno Labs

//! The last result of each of a connector's checks. See `crate::connectors::checks`.
//!
//! A run of a stored connector's checks replaces every result it reached, so the hub shows the
//! latest of each after a reload. A check a run did not reach keeps what it last found.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::upsert::excluded;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::connector_check_results;

#[derive(Debug, Clone, Queryable, Selectable, Insertable)]
#[diesel(table_name = connector_check_results, check_for_backend(diesel::pg::Pg))]
pub struct CheckResult {
    pub connector_id: Uuid,
    /// The check, as its kind names it, such as `sign_in`.
    pub check_id: String,
    /// `passed`, `failed`, or `skipped`.
    pub state: String,
    /// How many it found, for a check that counts.
    pub count: Option<i64>,
    /// Why, in Elysium's own terms, such as `no_messages`.
    pub reason: Option<String>,
    /// What the provider said.
    pub message: Option<String>,
    /// The check it waited on, when it was skipped because that one did not pass.
    pub blocked_by: Option<String>,
    pub ran_at: DateTime<Utc>,
}

/// Stores `results`, replacing what each check last found.
///
/// # Errors
/// Propagates any database error.
pub async fn record(
    connection: &mut AsyncPgConnection,
    results: &[CheckResult],
) -> QueryResult<()> {
    if results.is_empty() {
        return Ok(());
    }
    diesel::insert_into(connector_check_results::table)
        .values(results.to_vec())
        .on_conflict((
            connector_check_results::connector_id,
            connector_check_results::check_id,
        ))
        .do_update()
        .set((
            connector_check_results::state.eq(excluded(connector_check_results::state)),
            connector_check_results::count.eq(excluded(connector_check_results::count)),
            connector_check_results::reason.eq(excluded(connector_check_results::reason)),
            connector_check_results::message.eq(excluded(connector_check_results::message)),
            connector_check_results::blocked_by.eq(excluded(connector_check_results::blocked_by)),
            connector_check_results::ran_at.eq(excluded(connector_check_results::ran_at)),
        ))
        .execute(connection)
        .await?;
    Ok(())
}

/// The last results of every connector in `connector_ids`.
///
/// # Errors
/// Propagates any database error.
pub async fn list_for(
    connection: &mut AsyncPgConnection,
    connector_ids: &[Uuid],
) -> QueryResult<Vec<CheckResult>> {
    connector_check_results::table
        .filter(connector_check_results::connector_id.eq_any(connector_ids))
        .select(CheckResult::as_select())
        .load(connection)
        .await
}
