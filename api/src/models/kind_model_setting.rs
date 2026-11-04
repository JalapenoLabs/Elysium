// Copyright © 2026 Jalapeno Labs

//! The model and effort a kind of agent runs on, where someone chose one: one row per kind for
//! the workspace, and one per kind in each project that overrides it.
//!
//! A kind ([`AgentKind`]) groups every step and agent whose work is the same sort of turn, such
//! as the workers or the summaries. An admin sets a kind for the workspace; the people who edit
//! a project set it for that project's own agents, for the kinds whose turns run for one project
//! ([`AgentKind::runs_in_projects`]). A row exists only for a choice someone made; how the rows
//! combine with each agent's own setting is `crate::agent_models`, the one caller of [`set`] and
//! [`clear`].
//!
//! Every change is recorded in the workspace's history (`user_events`) with the setting before
//! and after, in the same transaction, as `model_setting` records an agent's.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde_json::json;
use uuid::Uuid;

use crate::agent_models::AgentKind;
use crate::database::schema::kind_model_settings;
use crate::models::model_setting::{ModelChoice, choice_from_row, history_value};
use crate::models::user::{self, UserEventKind};

/// Whose setting of a kind: the workspace's, or one project's override of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    Workspace,
    Project(Uuid),
}

impl Scope {
    /// The project the scope names, as `kind_model_settings.project_id` stores it.
    pub const fn project(self) -> Option<Uuid> {
        match self {
            Self::Workspace => None,
            Self::Project(project) => Some(project),
        }
    }
}

/// A kind's stored setting in one scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredKindSetting {
    pub kind: AgentKind,
    pub choice: ModelChoice,
    pub updated_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = kind_model_settings)]
struct SettingRow<'a> {
    id: Uuid,
    project_id: Option<Uuid>,
    kind: AgentKind,
    model: &'a str,
    effort: &'static str,
    created_by: Uuid,
}

/// Every kind's stored setting in `scope`, in no particular order.
///
/// # Errors
/// Propagates any database error.
pub async fn list(
    connection: &mut AsyncPgConnection,
    scope: Scope,
) -> QueryResult<Vec<StoredKindSetting>> {
    let rows: Vec<(AgentKind, String, String, DateTime<Utc>)> = kind_model_settings::table
        .filter(kind_model_settings::project_id.is_not_distinct_from(scope.project()))
        .select((
            kind_model_settings::kind,
            kind_model_settings::model,
            kind_model_settings::effort,
            kind_model_settings::updated_at,
        ))
        .load(connection)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(kind, model, effort, updated_at)| StoredKindSetting {
            kind,
            choice: choice_from_row(kind.name(), model, &effort),
            updated_at,
        })
        .collect())
}

/// Kind `kind`'s stored setting in `scope`, if someone chose one.
///
/// # Errors
/// Propagates any database error.
pub async fn find(
    connection: &mut AsyncPgConnection,
    scope: Scope,
    kind: AgentKind,
) -> QueryResult<Option<StoredKindSetting>> {
    let stored: Option<(String, String, DateTime<Utc>)> = kind_model_settings::table
        .filter(kind_model_settings::project_id.is_not_distinct_from(scope.project()))
        .filter(kind_model_settings::kind.eq(kind))
        .select((
            kind_model_settings::model,
            kind_model_settings::effort,
            kind_model_settings::updated_at,
        ))
        .first(connection)
        .await
        .optional()?;
    Ok(stored.map(|(model, effort, updated_at)| StoredKindSetting {
        kind,
        choice: choice_from_row(kind.name(), model, &effort),
        updated_at,
    }))
}

/// Sets what kind `kind` runs on in `scope`, as `actor` chose, recording the change. Answers
/// whether anything changed: choosing what is already stored records nothing.
///
/// # Errors
/// Propagates any database error, including a check violation for a project override of a kind
/// that runs in no project, and a foreign key violation for a project that does not exist.
pub async fn set(
    connection: &mut AsyncPgConnection,
    scope: Scope,
    kind: AgentKind,
    choice: &ModelChoice,
    actor: Uuid,
) -> QueryResult<bool> {
    connection
        .transaction(async move |connection| {
            let before = locked(connection, scope, kind).await?;
            if before.as_ref() == Some(choice) {
                return Ok(false);
            }
            diesel::insert_into(kind_model_settings::table)
                .values(SettingRow {
                    id: Uuid::now_v7(),
                    project_id: scope.project(),
                    kind,
                    model: &choice.model,
                    effort: choice.effort.as_str(),
                    created_by: actor,
                })
                .on_conflict((kind_model_settings::project_id, kind_model_settings::kind))
                .do_update()
                .set((
                    kind_model_settings::model.eq(&choice.model),
                    kind_model_settings::effort.eq(choice.effort.as_str()),
                ))
                .execute(connection)
                .await?;
            record_change(
                connection,
                scope,
                kind,
                before.as_ref(),
                Some(choice),
                actor,
            )
            .await?;
            Ok(true)
        })
        .await
}

/// Removes kind `kind`'s setting in `scope`, as `actor` asked, so it follows the level beneath
/// again, recording the change. Answers whether there was one to remove.
///
/// # Errors
/// Propagates any database error.
pub async fn clear(
    connection: &mut AsyncPgConnection,
    scope: Scope,
    kind: AgentKind,
    actor: Uuid,
) -> QueryResult<bool> {
    connection
        .transaction(async move |connection| {
            let Some(before) = locked(connection, scope, kind).await? else {
                return Ok(false);
            };
            diesel::delete(
                kind_model_settings::table
                    .filter(kind_model_settings::project_id.is_not_distinct_from(scope.project()))
                    .filter(kind_model_settings::kind.eq(kind)),
            )
            .execute(connection)
            .await?;
            record_change(connection, scope, kind, Some(&before), None, actor).await?;
            Ok(true)
        })
        .await
}

/// Kind `kind`'s stored setting in `scope`, its row locked until the transaction ends, so two
/// people's changes are recorded one after the other with the right values before each. Two
/// first settings racing each other meet at the unique constraint instead, and the upsert
/// lets the later one win.
async fn locked(
    connection: &mut AsyncPgConnection,
    scope: Scope,
    kind: AgentKind,
) -> QueryResult<Option<ModelChoice>> {
    let stored: Option<(String, String)> = kind_model_settings::table
        .filter(kind_model_settings::project_id.is_not_distinct_from(scope.project()))
        .filter(kind_model_settings::kind.eq(kind))
        .select((kind_model_settings::model, kind_model_settings::effort))
        .for_update()
        .first(connection)
        .await
        .optional()?;
    Ok(stored.map(|(model, effort)| choice_from_row(kind.name(), model, &effort)))
}

/// Records in the workspace's history that `actor` moved kind `kind` in `scope` from `before`
/// to `after`, either of which is `None` for what it follows when unset. Ids and names only, as
/// every history entry about a setting is.
async fn record_change(
    connection: &mut AsyncPgConnection,
    scope: Scope,
    kind: AgentKind,
    before: Option<&ModelChoice>,
    after: Option<&ModelChoice>,
    actor: Uuid,
) -> QueryResult<()> {
    user::record_event(
        connection,
        None,
        actor,
        UserEventKind::KindModelSettingChanged,
        json!({
            "kind": kind.name(),
            "projectId": scope.project(),
            "before": history_value(before),
            "after": history_value(after),
        }),
    )
    .await
}
