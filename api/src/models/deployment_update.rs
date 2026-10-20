// Copyright © 2026 Jalapeno Labs

//! The deployment's update history: every release the updater was told to apply, and
//! what came of it. See `crate::updates`.
//!
//! At most one update is applying at a time; a unique index enforces it, so two requests
//! racing to begin an update cannot both succeed.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::Serialize;
use uuid::Uuid;

use crate::database::schema::deployment_updates;

/// Where an update stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize)]
#[ExistingTypePath = "crate::database::schema::sql_types::DeploymentUpdateStatus"]
#[serde(rename_all = "kebab-case")]
pub enum DeploymentUpdateStatus {
    /// The updater was told to apply it and has not reported back.
    Applying,
    /// The release is running and healthy.
    Succeeded,
    /// The release did not come up healthy, and the updater restored the previous one.
    RolledBack,
}

/// A stored update.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = deployment_updates, check_for_backend(diesel::pg::Pg))]
pub struct DeploymentUpdate {
    pub id: Uuid,
    pub from_version: String,
    pub to_version: String,
    pub status: DeploymentUpdateStatus,
    /// Why it rolled back.
    pub reason: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    /// The admin who asked for it, or the system user for an automatic update.
    pub created_by: Uuid,
}

#[derive(Insertable)]
#[diesel(table_name = deployment_updates)]
struct NewDeploymentUpdate<'a> {
    id: Uuid,
    from_version: &'a str,
    to_version: &'a str,
    created_by: Uuid,
}

/// How an applying update ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateOutcome {
    Succeeded,
    /// With what went wrong.
    RolledBack(String),
}

/// Records that the updater was told to move from `from_version` to `to_version`.
///
/// # Errors
/// Fails with a unique violation when another update is already applying, and propagates
/// any other database error.
pub async fn begin(
    connection: &mut AsyncPgConnection,
    from_version: &str,
    to_version: &str,
    created_by: Uuid,
) -> QueryResult<DeploymentUpdate> {
    diesel::insert_into(deployment_updates::table)
        .values(NewDeploymentUpdate {
            id: Uuid::now_v7(),
            from_version,
            to_version,
            created_by,
        })
        .returning(DeploymentUpdate::as_returning())
        .get_result(connection)
        .await
}

/// The update that is applying, if one is.
///
/// # Errors
/// Propagates any database error.
pub async fn applying(connection: &mut AsyncPgConnection) -> QueryResult<Option<DeploymentUpdate>> {
    deployment_updates::table
        .filter(deployment_updates::status.eq(DeploymentUpdateStatus::Applying))
        .select(DeploymentUpdate::as_select())
        .first(connection)
        .await
        .optional()
}

/// Ends the applying update with `outcome`, when it is the one moving to `to_version`.
/// Returns `None` when no such update is applying, such as a report that arrives twice.
///
/// # Errors
/// Propagates any database error.
pub async fn finish(
    connection: &mut AsyncPgConnection,
    to_version: &str,
    outcome: UpdateOutcome,
) -> QueryResult<Option<DeploymentUpdate>> {
    let (status, reason) = match outcome {
        UpdateOutcome::Succeeded => (DeploymentUpdateStatus::Succeeded, None),
        UpdateOutcome::RolledBack(reason) => (DeploymentUpdateStatus::RolledBack, Some(reason)),
    };

    diesel::update(deployment_updates::table)
        .filter(deployment_updates::status.eq(DeploymentUpdateStatus::Applying))
        .filter(deployment_updates::to_version.eq(to_version))
        .set((
            deployment_updates::status.eq(status),
            deployment_updates::reason.eq(reason),
            deployment_updates::finished_at.eq(diesel::dsl::now),
        ))
        .returning(DeploymentUpdate::as_returning())
        .get_result(connection)
        .await
        .optional()
}

/// Every release that was applied and rolled back, so it is not offered again on its own.
///
/// # Errors
/// Propagates any database error.
pub async fn rolled_back_versions(connection: &mut AsyncPgConnection) -> QueryResult<Vec<String>> {
    deployment_updates::table
        .filter(deployment_updates::status.eq(DeploymentUpdateStatus::RolledBack))
        .select(deployment_updates::to_version)
        .distinct()
        .load(connection)
        .await
}

/// The latest `limit` updates, newest first.
///
/// # Errors
/// Propagates any database error.
pub async fn recent(
    connection: &mut AsyncPgConnection,
    limit: i64,
) -> QueryResult<Vec<DeploymentUpdate>> {
    deployment_updates::table
        .order(deployment_updates::id.desc())
        .limit(limit)
        .select(DeploymentUpdate::as_select())
        .load(connection)
        .await
}
