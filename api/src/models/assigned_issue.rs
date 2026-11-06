// Copyright © 2026 Jalapeno Labs

//! The issues each issue connector brought in because they are assigned to its person, one row
//! per connector and issue, naming the item it raised or found already linked.
//!
//! The row is what keeps an issue from raising a second item, so it is written in the same
//! transaction as the item and its link, and it outlives both: an item dismissed, deleted, or
//! unlinked from its issue is not raised again. See `crate::action_items::assigned`.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::assigned_issues;

/// An issue a connector is bringing in.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = assigned_issues)]
pub struct NewAssignedIssue {
    pub id: Uuid,
    pub connector_id: Uuid,
    /// The issue as its provider names it for good, as a link's `external_id` holds it.
    pub external_id: String,
    /// The item it raised, or the one already linked to it.
    pub action_item_id: Uuid,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

/// Every issue `connector_id` has brought in, by external id.
///
/// # Errors
/// Propagates any database error.
pub async fn external_ids(
    connection: &mut AsyncPgConnection,
    connector_id: Uuid,
) -> QueryResult<HashSet<String>> {
    let ids: Vec<String> = assigned_issues::table
        .filter(assigned_issues::connector_id.eq(connector_id))
        .select(assigned_issues::external_id)
        .load(connection)
        .await?;
    Ok(ids.into_iter().collect())
}

/// Records that a connector brought an issue in. Answers `false` when it already had, in which
/// case nothing is written.
///
/// # Errors
/// Propagates any database error.
pub async fn record(
    connection: &mut AsyncPgConnection,
    issue: NewAssignedIssue,
) -> QueryResult<bool> {
    let inserted = diesel::insert_into(assigned_issues::table)
        .values(issue)
        .on_conflict((assigned_issues::connector_id, assigned_issues::external_id))
        .do_nothing()
        .execute(connection)
        .await?;
    Ok(inserted == 1)
}
