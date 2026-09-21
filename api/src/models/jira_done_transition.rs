// Copyright © 2026 Jalapeno Labs

//! The done transition chosen for a Jira project, stored as the `done` status it leads
//! into, per credential.
//!
//! A project with exactly one status in the `done` category needs no choice: resolving an
//! item moves its issue into that status. Only a choice the user made among several is
//! stored here. The status is stored rather than a transition id because a workflow names
//! a different transition into the same status from each status an issue can be in. See
//! `docs/jira.md`.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::upsert::excluded;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::jira_done_transitions;

/// A stored choice.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = jira_done_transitions, check_for_backend(diesel::pg::Pg))]
pub struct JiraDoneTransition {
    pub jira_credential_id: Uuid,
    pub project_key: String,
    /// The `done` status the transition leads into.
    pub status_id: String,
    pub status_name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = jira_done_transitions)]
struct ChoiceRow<'a> {
    jira_credential_id: Uuid,
    project_key: &'a str,
    status_id: &'a str,
    status_name: &'a str,
}

/// The choice made for one project, if any.
///
/// # Errors
/// Propagates any database error.
pub async fn find(
    connection: &mut AsyncPgConnection,
    jira_credential_id: Uuid,
    project_key: &str,
) -> QueryResult<Option<JiraDoneTransition>> {
    jira_done_transitions::table
        .find((jira_credential_id, project_key))
        .select(JiraDoneTransition::as_select())
        .first(connection)
        .await
        .optional()
}

/// Chooses the status a project's done transition leads into, replacing any earlier
/// choice.
///
/// # Errors
/// Propagates any database error, including a foreign key violation for an unknown
/// credential.
pub async fn choose(
    connection: &mut AsyncPgConnection,
    jira_credential_id: Uuid,
    project_key: &str,
    status_id: &str,
    status_name: &str,
) -> QueryResult<JiraDoneTransition> {
    diesel::insert_into(jira_done_transitions::table)
        .values(ChoiceRow {
            jira_credential_id,
            project_key,
            status_id,
            status_name,
        })
        .on_conflict((
            jira_done_transitions::jira_credential_id,
            jira_done_transitions::project_key,
        ))
        .do_update()
        .set((
            jira_done_transitions::status_id.eq(excluded(jira_done_transitions::status_id)),
            jira_done_transitions::status_name.eq(excluded(jira_done_transitions::status_name)),
        ))
        .returning(JiraDoneTransition::as_returning())
        .get_result(connection)
        .await
}

/// Forgets a project's choice, so the project asks again unless it has one done status.
///
/// # Errors
/// Propagates any database error.
pub async fn forget(
    connection: &mut AsyncPgConnection,
    jira_credential_id: Uuid,
    project_key: &str,
) -> QueryResult<()> {
    diesel::delete(jira_done_transitions::table.find((jira_credential_id, project_key)))
        .execute(connection)
        .await?;
    Ok(())
}
