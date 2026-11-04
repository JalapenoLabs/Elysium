// Copyright © 2026 Jalapeno Labs

//! The tracker a project names for the issues its Project Manager files, overriding the
//! precedent of the issues its items already link (`crate::project_managers::trackers`).
//!
//! A tracker is a provider, the place on it issues open (a Jira project key, a GitHub
//! `owner/name`, or a YouTrack project's short name), and the credential that files there: a
//! Jira credential or a YouTrack account. A GitHub tracker names none, since GitHub writes go
//! through the token the project's sessions get. Deleting the credential deletes the tracker,
//! and the project follows its precedent again.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::project_issue_trackers;
use crate::models::action_item_link::LinkProvider;

/// A project's named tracker.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(
    table_name = project_issue_trackers,
    primary_key(project_id),
    check_for_backend(diesel::pg::Pg)
)]
pub struct NamedTracker {
    pub project_id: Uuid,
    pub provider: LinkProvider,
    pub jira_credential_id: Option<Uuid>,
    pub token_account_id: Option<Uuid>,
    pub destination: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl NamedTracker {
    /// The credential it names, for a provider whose tracker names one.
    pub fn credential_id(&self) -> Option<Uuid> {
        self.jira_credential_id.or(self.token_account_id)
    }
}

/// A tracker to name.
#[derive(Debug, Clone, Insertable, AsChangeset)]
#[diesel(table_name = project_issue_trackers, treat_none_as_null = true)]
pub struct NewTracker {
    pub project_id: Uuid,
    pub provider: LinkProvider,
    pub jira_credential_id: Option<Uuid>,
    pub token_account_id: Option<Uuid>,
    pub destination: String,
    pub created_by: Uuid,
}

/// The tracker `project_id` names, if it names one.
///
/// # Errors
/// Propagates any database error.
pub async fn find(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
) -> QueryResult<Option<NamedTracker>> {
    project_issue_trackers::table
        .find(project_id)
        .select(NamedTracker::as_select())
        .first(connection)
        .await
        .optional()
}

/// Names `tracker` for its project, replacing any it named before.
///
/// # Errors
/// Propagates any database error, including a foreign key violation for a credential or a
/// project that does not exist.
pub async fn set(
    connection: &mut AsyncPgConnection,
    tracker: &NewTracker,
) -> QueryResult<NamedTracker> {
    diesel::insert_into(project_issue_trackers::table)
        .values(tracker)
        .on_conflict(project_issue_trackers::project_id)
        .do_update()
        .set(tracker)
        .returning(NamedTracker::as_returning())
        .get_result(connection)
        .await
}

/// Stops `project_id` naming a tracker, so it follows its precedent. Answers whether it named
/// one.
///
/// # Errors
/// Propagates any database error.
pub async fn clear(connection: &mut AsyncPgConnection, project_id: Uuid) -> QueryResult<bool> {
    let deleted = diesel::delete(project_issue_trackers::table.find(project_id))
        .execute(connection)
        .await?;
    Ok(deleted > 0)
}
