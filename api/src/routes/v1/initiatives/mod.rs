// Copyright © 2026 Jalapeno Labs

//! `/api/v1/initiatives`: goals that end, grouping action items and carrying progress.
//!
//! Items join and leave initiatives through `/api/v1/action-items/{id}/initiatives`. Every
//! write here is the user acting, recorded in the initiative's history as `user`, and
//! published on the event stream with its progress. See `docs/action-items.md`.

mod add_project;
mod create_initiative;
mod delete_initiative;
mod get_initiative;
mod get_progress;
mod list_history;
mod list_initiatives;
mod remove_project;
mod restore_initiative;
mod update_initiative;

use anyhow::Context;
use axum::Router;
use axum::routing::{get, post, put};
use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use serde::Serialize;
use uuid::Uuid;

use crate::action_items::progress::{self, Progress};
use crate::errors::ApiError;
use crate::models::action_item::{self, ActionItemFilter};
use crate::models::action_item_event::Recorded;
use crate::models::initiative::{self, Initiative, InitiativeState};
use crate::realtime::ServerEvent;
use crate::routes::v1::action_items::{item_responses, publish_history};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(list_initiatives::handle).post(create_initiative::handle),
        )
        .route(
            "/{id}",
            get(get_initiative::handle)
                .patch(update_initiative::handle)
                .delete(delete_initiative::handle),
        )
        .route("/{id}/restore", post(restore_initiative::handle))
        .route("/{id}/progress", get(get_progress::handle))
        .route("/{id}/history", get(list_history::handle))
        .route(
            "/{id}/projects/{project_id}",
            put(add_project::handle).delete(remove_project::handle),
        )
}

/// An initiative as clients see it, with its progress now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitiativeResponse {
    id: Uuid,
    name: String,
    description: String,
    state: InitiativeState,
    target_at: Option<DateTime<Utc>>,
    project_ids: Vec<Uuid>,
    progress: Progress,
    deleted_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// `initiatives` as clients see them, each with its projects and its progress at `now`.
///
/// # Errors
/// Propagates any database error.
pub async fn initiative_responses(
    connection: &mut AsyncPgConnection,
    initiatives: Vec<Initiative>,
    now: DateTime<Utc>,
) -> Result<Vec<InitiativeResponse>, ApiError> {
    let ids: Vec<Uuid> = initiatives.iter().map(|initiative| initiative.id).collect();
    let mut project_ids = initiative::project_ids(connection, &ids).await?;
    let memberships = initiative::memberships(connection, &ids).await?;

    Ok(initiatives
        .into_iter()
        .map(|initiative| {
            let spans = memberships
                .get(&initiative.id)
                .map_or(&[][..], Vec::as_slice);
            InitiativeResponse {
                progress: progress::at(spans, now),
                project_ids: project_ids.remove(&initiative.id).unwrap_or_default(),
                id: initiative.id,
                name: initiative.name,
                description: initiative.description,
                state: initiative.state,
                target_at: initiative.target_at,
                deleted_at: initiative.deleted_at,
                created_at: initiative.created_at,
                updated_at: initiative.updated_at,
            }
        })
        .collect())
}

/// After a write to an initiative: publishes the history it recorded and the initiative
/// (as deleted when it is), and answers it as clients see it. A write that changed nothing
/// publishes nothing.
///
/// # Errors
/// Propagates any database error.
pub async fn publish_initiative_write(
    state: &AppState,
    connection: &mut AsyncPgConnection,
    written: Recorded<Initiative>,
    now: DateTime<Utc>,
) -> Result<InitiativeResponse, ApiError> {
    let Recorded { record, history } = written;
    let id = record.id;
    let is_deleted = record.deleted_at.is_some();
    let response = initiative_responses(connection, vec![record], now)
        .await?
        .pop()
        .context("one initiative in, one response out")?;
    if history.is_empty() {
        return Ok(response);
    }

    publish_history(state, history);
    if is_deleted {
        state.events.publish(&ServerEvent::InitiativeDeleted { id });
    } else {
        state
            .events
            .publish(&ServerEvent::InitiativeUpserted(response.clone()));
    }
    Ok(response)
}

/// Publishes every live item in the initiative. Items list only initiatives that are not
/// deleted, so deleting or restoring one changes each of its items.
///
/// # Errors
/// Propagates any database error.
pub async fn publish_member_items(
    state: &AppState,
    connection: &mut AsyncPgConnection,
    initiative_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), ApiError> {
    let filter = ActionItemFilter {
        initiative: Some(initiative_id),
        ..ActionItemFilter::default()
    };
    let items = action_item::list(connection, &filter, now).await?;
    for item in item_responses(connection, items).await? {
        state.events.publish(&ServerEvent::ActionItemUpserted(item));
    }
    Ok(())
}
