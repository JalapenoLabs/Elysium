// Copyright © 2026 Jalapeno Labs

//! The versions of the agent guidance, every one kept: the newest is what agents read.
//!
//! A version is never changed once written. A save and a restore each add the next one, so the
//! table is the guidance's history and names who wrote each version. What a version means to an
//! agent, and the rules added to it, are `crate::agent_guidance`'s; this module only stores them.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::agent_guidance_versions;

/// One version of the guidance, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = agent_guidance_versions, check_for_backend(diesel::pg::Pg))]
pub struct GuidanceVersion {
    /// 1 for the seed its migration wrote, then one more for each save and each restore.
    pub version: i32,
    /// Markdown, trimmed; empty when the company adds nothing to Elysium's rules.
    pub content: String,
    /// The version a restore copied the text of.
    pub restored_from: Option<i32>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

/// A version to add after the newest.
#[derive(Debug, Insertable)]
#[diesel(table_name = agent_guidance_versions)]
pub struct NewVersion<'a> {
    pub version: i32,
    pub content: &'a str,
    pub restored_from: Option<i32>,
    pub created_by: Uuid,
}

/// The newest version, which every agent reads. One always exists: the migration seeds the first.
///
/// # Errors
/// Propagates any database error.
pub async fn current(connection: &mut AsyncPgConnection) -> QueryResult<GuidanceVersion> {
    agent_guidance_versions::table
        .order(agent_guidance_versions::version.desc())
        .select(GuidanceVersion::as_select())
        .first(connection)
        .await
}

/// Every version, newest first.
///
/// # Errors
/// Propagates any database error.
pub async fn versions(connection: &mut AsyncPgConnection) -> QueryResult<Vec<GuidanceVersion>> {
    agent_guidance_versions::table
        .order(agent_guidance_versions::version.desc())
        .select(GuidanceVersion::as_select())
        .load(connection)
        .await
}

/// The version numbered `version`.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when there is none; any other database error.
pub async fn find(
    connection: &mut AsyncPgConnection,
    version: i32,
) -> QueryResult<GuidanceVersion> {
    agent_guidance_versions::table
        .find(version)
        .select(GuidanceVersion::as_select())
        .first(connection)
        .await
}

/// Adds `new`, answering it as stored.
///
/// # Errors
/// A unique violation when a version with its number was written meanwhile, which is how two
/// admins saving at once are told apart: the second is refused rather than numbered past the
/// first. Any other database error.
pub async fn insert(
    connection: &mut AsyncPgConnection,
    new: &NewVersion<'_>,
) -> QueryResult<GuidanceVersion> {
    diesel::insert_into(agent_guidance_versions::table)
        .values(new)
        .returning(GuidanceVersion::as_returning())
        .get_result(connection)
        .await
}
