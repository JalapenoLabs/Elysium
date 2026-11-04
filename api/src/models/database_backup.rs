// Copyright © 2026 Jalapeno Labs

//! The history of database backups: every attempt, what started it, and how it ended. See
//! `crate::backups`.
//!
//! At most one backup runs at a time; a unique index enforces it, so two triggers racing to
//! begin one cannot both succeed.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::Serialize;
use uuid::Uuid;

use crate::database::schema::database_backups;

/// Where a backup stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize)]
#[ExistingTypePath = "crate::database::schema::sql_types::DatabaseBackupStatus"]
#[serde(rename_all = "kebab-case")]
pub enum DatabaseBackupStatus {
    /// Dumping and uploading.
    Running,
    /// Every database's dump is stored in the location.
    Succeeded,
    /// Something failed; what the attempt uploaded was removed again.
    Failed,
}

/// What started a backup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize)]
#[ExistingTypePath = "crate::database::schema::sql_types::DatabaseBackupTrigger"]
#[serde(rename_all = "kebab-case")]
pub enum DatabaseBackupTrigger {
    /// The nightly schedule.
    Schedule,
    /// An admin's Back up now.
    Admin,
    /// The deployment's own tooling, before it changes the stack.
    Deployment,
}

/// A stored backup attempt.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = database_backups, check_for_backend(diesel::pg::Pg))]
pub struct DatabaseBackup {
    pub id: Uuid,
    pub trigger: DatabaseBackupTrigger,
    pub status: DatabaseBackupStatus,
    /// Where the files went; `None` once that location was deleted.
    pub storage_location_id: Option<Uuid>,
    /// To the second, since it names the backup's files.
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    /// Every dump together, once it succeeded.
    pub size_bytes: Option<i64>,
    /// What went wrong, once it failed.
    pub error: Option<String>,
    pub created_by: Uuid,
    /// When the engine took it. A backup someone asked for waits, running but untaken, until
    /// the engine does.
    pub claimed_at: Option<DateTime<Utc>>,
}

#[derive(Insertable)]
#[diesel(table_name = database_backups)]
struct NewDatabaseBackup {
    id: Uuid,
    trigger: DatabaseBackupTrigger,
    storage_location_id: Uuid,
    started_at: DateTime<Utc>,
    created_by: Uuid,
    claimed_at: Option<DateTime<Utc>>,
}

/// How a running backup ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupOutcome {
    /// With the size of every dump together.
    Succeeded { size_bytes: i64 },
    /// With what went wrong.
    Failed(String),
}

/// The longest error kept, matching the column's limit.
const MAX_ERROR_CHARACTERS: usize = 2000;

/// Records that a backup to `storage_location_id` began at `started_at`: taken by the engine
/// at once when `claimed`, or waiting for it when someone asked for it elsewhere.
///
/// # Errors
/// Fails with a unique violation when another backup is already running, and propagates any
/// other database error.
pub async fn begin(
    connection: &mut AsyncPgConnection,
    trigger: DatabaseBackupTrigger,
    storage_location_id: Uuid,
    started_at: DateTime<Utc>,
    created_by: Uuid,
    claimed: bool,
) -> QueryResult<DatabaseBackup> {
    diesel::insert_into(database_backups::table)
        .values(NewDatabaseBackup {
            id: Uuid::now_v7(),
            trigger,
            storage_location_id,
            started_at,
            created_by,
            claimed_at: claimed.then(Utc::now),
        })
        .returning(DatabaseBackup::as_returning())
        .get_result(connection)
        .await
}

/// Ends the running backup `id` with `outcome`. Returns `None` when it is not running, such
/// as one a restart already settled.
///
/// # Errors
/// Propagates any database error.
pub async fn finish(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    outcome: BackupOutcome,
) -> QueryResult<Option<DatabaseBackup>> {
    let (status, size_bytes, error) = match outcome {
        BackupOutcome::Succeeded { size_bytes } => {
            (DatabaseBackupStatus::Succeeded, Some(size_bytes), None)
        }
        BackupOutcome::Failed(error) => {
            let error: String = error.chars().take(MAX_ERROR_CHARACTERS).collect();
            (DatabaseBackupStatus::Failed, None, Some(error))
        }
    };

    diesel::update(database_backups::table)
        .filter(database_backups::id.eq(id))
        .filter(database_backups::status.eq(DatabaseBackupStatus::Running))
        .set((
            database_backups::status.eq(status),
            database_backups::size_bytes.eq(size_bytes),
            database_backups::error.eq(error),
            database_backups::finished_at.eq(diesel::dsl::now),
        ))
        .returning(DatabaseBackup::as_returning())
        .get_result(connection)
        .await
        .optional()
}

/// Takes the backup someone asked for that the engine has not taken yet, if there is one.
///
/// # Errors
/// Propagates any database error.
pub async fn claim_requested(
    connection: &mut AsyncPgConnection,
) -> QueryResult<Option<DatabaseBackup>> {
    diesel::update(database_backups::table)
        .filter(database_backups::status.eq(DatabaseBackupStatus::Running))
        .filter(database_backups::claimed_at.is_null())
        .set(database_backups::claimed_at.eq(diesel::dsl::now))
        .returning(DatabaseBackup::as_returning())
        .get_result(connection)
        .await
        .optional()
}

/// Fails every backup the engine had taken and not finished, with `error`. Only an engine that
/// stopped mid-backup leaves one, so this runs once as it starts, before it takes another. A
/// backup still waiting to be taken is left: it is still owed.
///
/// # Errors
/// Propagates any database error.
pub async fn fail_running(
    connection: &mut AsyncPgConnection,
    error: &str,
) -> QueryResult<Vec<DatabaseBackup>> {
    diesel::update(database_backups::table)
        .filter(database_backups::status.eq(DatabaseBackupStatus::Running))
        .filter(database_backups::claimed_at.is_not_null())
        .set((
            database_backups::status.eq(DatabaseBackupStatus::Failed),
            database_backups::error.eq(error),
            database_backups::finished_at.eq(diesel::dsl::now),
        ))
        .returning(DatabaseBackup::as_returning())
        .get_results(connection)
        .await
}

/// Every backup that began at or after `since`, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn since(
    connection: &mut AsyncPgConnection,
    since: DateTime<Utc>,
) -> QueryResult<Vec<DatabaseBackup>> {
    database_backups::table
        .filter(database_backups::started_at.ge(since))
        .order(database_backups::started_at.asc())
        .select(DatabaseBackup::as_select())
        .load(connection)
        .await
}

/// The latest `limit` backups, newest first.
///
/// # Errors
/// Propagates any database error.
pub async fn recent(
    connection: &mut AsyncPgConnection,
    limit: i64,
) -> QueryResult<Vec<DatabaseBackup>> {
    database_backups::table
        .order(database_backups::started_at.desc())
        .limit(limit)
        .select(DatabaseBackup::as_select())
        .load(connection)
        .await
}

/// One backup, by id.
///
/// # Errors
/// Propagates any database error.
pub async fn find(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> QueryResult<Option<DatabaseBackup>> {
    database_backups::table
        .find(id)
        .select(DatabaseBackup::as_select())
        .first(connection)
        .await
        .optional()
}
