// Copyright © 2026 Jalapeno Labs

//! Syncs worth showing: the reads of a connector that recorded something, and the ones that
//! failed. They make the hub's activity feed and each connector's last sync. See
//! `docs/connectors.md`.
//!
//! A read that found nothing new is not recorded: a mailbox is read every minute, and a row
//! for each would bury what matters. Each connector keeps its newest [`RUNS_KEPT`] runs.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::connector_runs;

/// How many runs each connector keeps. Enough for a week of a busy mailbox's changes, few
/// enough that the activity feed stays one cheap query.
const RUNS_KEPT: i64 = 50;

/// How many words of a failure a run keeps; the column holds 2048 characters.
const ERROR_MAX_CHARACTERS: usize = 2048;

#[derive(Debug, Clone, Queryable, Selectable, Insertable)]
#[diesel(table_name = connector_runs, check_for_backend(diesel::pg::Pg))]
pub struct ConnectorRun {
    pub id: Uuid,
    pub connector_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    /// `succeeded` or `failed`.
    pub outcome: String,
    /// How many things it recorded: timeline records, or changes to linked issues.
    pub records: i32,
    pub error: Option<String>,
}

impl ConnectorRun {
    /// A read of `connector_id` that started at `started_at` and recorded `records` things.
    pub fn succeeded(connector_id: Uuid, started_at: DateTime<Utc>, records: usize) -> Self {
        Self {
            id: Uuid::now_v7(),
            connector_id,
            started_at,
            finished_at: Utc::now(),
            outcome: "succeeded".to_owned(),
            records: i32::try_from(records).unwrap_or(i32::MAX),
            error: None,
        }
    }

    /// A read of `connector_id` that started at `started_at` and failed with `error`.
    pub fn failed(connector_id: Uuid, started_at: DateTime<Utc>, error: &str) -> Self {
        Self {
            id: Uuid::now_v7(),
            connector_id,
            started_at,
            finished_at: Utc::now(),
            outcome: "failed".to_owned(),
            records: 0,
            error: Some(error.chars().take(ERROR_MAX_CHARACTERS).collect()),
        }
    }
}

/// Stores `run`, then drops whatever its connector holds past its newest [`RUNS_KEPT`].
///
/// # Errors
/// Propagates any database error.
pub async fn record(connection: &mut AsyncPgConnection, run: ConnectorRun) -> QueryResult<()> {
    let connector_id = run.connector_id;
    diesel::insert_into(connector_runs::table)
        .values(run)
        .execute(connection)
        .await?;

    let kept: Vec<Uuid> = connector_runs::table
        .filter(connector_runs::connector_id.eq(connector_id))
        .order(connector_runs::started_at.desc())
        .limit(RUNS_KEPT)
        .select(connector_runs::id)
        .load(connection)
        .await?;
    diesel::delete(
        connector_runs::table
            .filter(connector_runs::connector_id.eq(connector_id))
            .filter(connector_runs::id.ne_all(kept)),
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// The newest `limit` runs of any connector in `connector_ids`, newest first.
///
/// # Errors
/// Propagates any database error.
pub async fn recent(
    connection: &mut AsyncPgConnection,
    connector_ids: &[Uuid],
    limit: i64,
) -> QueryResult<Vec<ConnectorRun>> {
    connector_runs::table
        .filter(connector_runs::connector_id.eq_any(connector_ids))
        .order(connector_runs::started_at.desc())
        .limit(limit)
        .select(ConnectorRun::as_select())
        .load(connection)
        .await
}

/// The newest run of each connector in `connector_ids`.
///
/// # Errors
/// Propagates any database error.
pub async fn latest_each(
    connection: &mut AsyncPgConnection,
    connector_ids: &[Uuid],
) -> QueryResult<Vec<ConnectorRun>> {
    connector_runs::table
        .filter(connector_runs::connector_id.eq_any(connector_ids))
        .distinct_on(connector_runs::connector_id)
        .order((
            connector_runs::connector_id,
            connector_runs::started_at.desc(),
        ))
        .select(ConnectorRun::as_select())
        .load(connection)
        .await
}
