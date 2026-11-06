// Copyright © 2026 Jalapeno Labs

//! Workers and Project Managers run only with `~/.claude` from the configuration repository.
//!
//! The configuration repository holds what does more for an agent's work than anything else
//! Elysium gives it: the instructions, docs, manuals, skills, and learnings. So no worker or
//! Project Manager turn starts without it. [`hold`] says whether they may start, from the
//! workspace settings and how the last fetch went (`crate::models::agent_configuration`):
//!
//! | Settings and last fetch                                       | [`Hold`]       |
//! |---------------------------------------------------------------|----------------|
//! | No repository set                                             | `Unset`        |
//! | A repository or token the last fetch was not about            | `Fetching`     |
//! | The last fetch of this repository and token failed            | `Failed`       |
//! | The last fetch of this repository and token checked a commit out | none: agents start |
//!
//! The worker pool's scheduler hands out no slot while there is a hold, so no batch holds a
//! slot it cannot use, and the Project Managers' dispatcher starts no turn; their wakes wait.
//! When either holds back work that is waiting, [`raise_hold`] raises one inbox item, never one
//! per batch or project; once a fetch succeeds, [`resolve_hold`] resolves it.
//!
//! Coding sessions, Studio items, and Elysia are not held: a person starts each session and
//! watches it, and Elysia is who tells the person that work is held back, so holding her would
//! silence the message. They run on whatever checkout the shared workspace has.
//!
//! The engine fetches the repository into the shared workspace ([`keeper`]) as it starts, when
//! an admin changes the repository or its token or asks for a fetch, and every
//! [`keeper::RETRY_INTERVAL`] while a hold remains. Every worker fetches it again into its own
//! container; one whose fetch fails records the failure here, which holds the pool until the
//! engine's next fetch succeeds.

pub mod keeper;

use chrono::{DateTime, Utc};
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde::Serialize;
use tracing::{Level, event};
use uuid::Uuid;

use crate::action_items::{Actor, Transition, WorkError};
use crate::models::action_item::{
    self, ActionItem, ActionItemChanges, ActionItemPriority, ActionItemState, NewActionItem, Owner,
};
use crate::models::action_item_event::Recorded;
use crate::models::agent_configuration::{self as status_record, Status};
use crate::models::project_manager;
use crate::models::user::{self, WorkspaceSettings};
use crate::realtime::{EventBus, ServerEvent};
use crate::routes::v1::action_items::publish_item_write;
use crate::routes::v1::workspace_settings::WorkspaceSettingsResponse;

/// Why workers and Project Managers may not start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "reason",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Hold {
    /// No configuration repository is set.
    Unset,
    /// The repository or its token changed, and the engine has not fetched it yet.
    Fetching,
    /// The last fetch of the repository failed.
    Failed {
        error: String,
        failed_at: DateTime<Utc>,
    },
}

/// Whether agents that need the configuration repository may start, given the workspace
/// `settings` and the last fetch's `status`; `None` when they may.
pub fn hold(settings: &WorkspaceSettings, status: &Status) -> Option<Hold> {
    let Some(source) = status_record::named_source(settings) else {
        return Some(Hold::Unset);
    };
    if !status.describes(&source) {
        return Some(Hold::Fetching);
    }
    if let (Some(error), Some(failed_at)) = (&status.error, status.failed_at) {
        return Some(Hold::Failed {
            error: error.clone(),
            failed_at,
        });
    }
    if status.commit_sha.is_none() {
        return Some(Hold::Fetching);
    }
    None
}

/// [`hold`] as the database stands.
///
/// # Errors
/// Propagates any database error.
pub async fn current_hold(connection: &mut AsyncPgConnection) -> diesel::QueryResult<Option<Hold>> {
    let settings = user::workspace_settings(connection).await?;
    let status = status_record::find(connection).await?;
    Ok(hold(&settings, &status))
}

/// Raises the inbox item that says work is held back by `hold`. One raised before that is still
/// in the inbox or open is kept, its title and notes brought up to `hold`, so the one item always
/// says why work waits now, such as after the repository changed while it was failing. A hold that
/// is only the engine fetching raises nothing: it lifts in moments on its own. Called by whoever
/// holds work back, as it does.
///
/// # Errors
/// Propagates any database error.
pub async fn raise_hold(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    hold: &Hold,
) -> anyhow::Result<()> {
    let Some(item) = item_for(hold) else {
        return Ok(());
    };
    let now = Utc::now();
    let raised = connection
        .transaction(async |connection| {
            let status = status_record::lock(connection).await?;
            if let Some(raised) = status.inbox_item_id {
                let existing = action_item::find(connection, raised).await?;
                if is_waiting_for_someone(&existing) {
                    // The same words change nothing and record nothing.
                    let changes = ActionItemChanges {
                        title: Some(item.title),
                        notes: Some(item.notes),
                        ..ActionItemChanges::default()
                    };
                    let updated =
                        action_item::update(connection, raised, changes, Actor::System, now)
                            .await?;
                    return Ok::<_, WorkError>((!updated.history.is_empty()).then_some(updated));
                }
            }
            let created = action_item::create(connection, item, Actor::System, now).await?;
            status_record::set_inbox_item(connection, Some(created.record.id)).await?;
            Ok(Some(created))
        })
        .await
        .map_err(|error| anyhow::anyhow!("cannot raise an inbox item: {error}"))?;
    let Some(raised) = raised else {
        return Ok(());
    };
    event!(
        name: "agent_configuration.hold.raised",
        Level::INFO,
        action_item.id = %raised.record.id,
        "raised or updated the inbox item for work the configuration repository holds back",
    );
    publish(events, connection, raised, now).await
}

/// Resolves the inbox item [`raise_hold`] raised, now that nothing holds work back, unless the
/// person dealt with it first.
///
/// # Errors
/// Propagates any database error.
pub async fn resolve_hold(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
) -> anyhow::Result<()> {
    let now = Utc::now();
    let resolved = connection
        .transaction(async |connection| {
            let status = status_record::lock(connection).await?;
            let Some(raised) = status.inbox_item_id else {
                return Ok::<_, WorkError>(None);
            };
            status_record::set_inbox_item(connection, None).await?;
            match action_item::transition(
                connection,
                raised,
                Transition::Resolve,
                Actor::System,
                now,
            )
            .await
            {
                Ok(resolved) => Ok(Some(resolved)),
                Err(WorkError::Database(error)) => Err(WorkError::Database(error)),
                // Resolved, dismissed, or deleted already: the person dealt with it.
                Err(_refused) => Ok(None),
            }
        })
        .await
        .map_err(|error| anyhow::anyhow!("cannot resolve the inbox item: {error}"))?;
    let Some(resolved) = resolved else {
        return Ok(());
    };
    event!(
        name: "agent_configuration.hold.resolved",
        Level::INFO,
        action_item.id = %resolved.record.id,
        "resolved the inbox item for held-back work: the configuration repository was fetched",
    );
    publish(events, connection, resolved, now).await
}

/// Tells every client what changed after a fetch's outcome was recorded: the settings page shows
/// the commit or the failure, the work queue and each Project Manager with waiting work the hold.
///
/// # Errors
/// Propagates any database error.
pub async fn announce(connection: &mut AsyncPgConnection, events: &EventBus) -> anyhow::Result<()> {
    let settings = user::workspace_settings(connection).await?;
    let status = status_record::find(connection).await?;
    events.publish(&ServerEvent::WorkspaceSettingsUpdated(
        WorkspaceSettingsResponse::new(settings, &status),
    ));
    events.publish(&ServerEvent::WorkerPoolChanged);
    let mut projects: Vec<Uuid> = project_manager::waiting(connection)
        .await?
        .into_iter()
        .map(|wake| wake.project_id)
        .collect();
    projects.sort_unstable();
    projects.dedup();
    for project_id in projects {
        events.publish(&ServerEvent::ProjectManagerChanged { project_id });
    }
    Ok(())
}

/// The inbox item `hold` raises, if any.
fn item_for(hold: &Hold) -> Option<NewActionItem> {
    let (title, notes) = match hold {
        Hold::Fetching => return None,
        Hold::Unset => (
            "Choose a configuration repository for agents".to_owned(),
            "Workers and Project Managers run only with `~/.claude` from the configuration \
             repository, and none is set, so their work waits. An admin chooses one, and the \
             GitHub token that reads it, under Settings, LLMs, Agent configuration."
                .to_owned(),
        ),
        Hold::Failed { error, .. } => (
            "The configuration repository could not be fetched".to_owned(),
            format!(
                "Workers and Project Managers run only with `~/.claude` from the configuration \
                 repository, which could not be fetched, so their work waits. Elysium tries \
                 again every few minutes, and Settings, LLMs, Agent configuration fetches it at \
                 once.\n\nThe last fetch said: {error}"
            ),
        ),
    };
    Some(NewActionItem {
        title,
        notes,
        state: ActionItemState::Inbox,
        priority: ActionItemPriority::High,
        due_at: None,
        owner: Owner::User,
        project_ids: Vec::new(),
        initiative_ids: Vec::new(),
    })
}

/// Whether `item` still waits for someone to deal with it.
fn is_waiting_for_someone(item: &ActionItem) -> bool {
    item.deleted_at.is_none()
        && matches!(item.state, ActionItemState::Inbox | ActionItemState::Open)
}

async fn publish(
    events: &EventBus,
    connection: &mut AsyncPgConnection,
    written: Recorded<ActionItem>,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    publish_item_write(events, connection, written, &[], now)
        .await
        .map(drop)
        .map_err(|error| anyhow::anyhow!("cannot publish the item: {error}"))
}

#[cfg(test)]
mod tests;
