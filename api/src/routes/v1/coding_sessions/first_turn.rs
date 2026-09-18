// Copyright © 2026 Jalapeno Labs

//! A new session's first turn: the prompt it was created with, and for a session started
//! from an action item, the item's context ahead of it.

use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use uuid::Uuid;

use crate::action_items::progress;
use crate::action_items::session_context::{self, ItemContext};
use crate::errors::ApiError;
use crate::models::action_item_comment;
use crate::models::project::Project;
use crate::models::{action_item, initiative};

/// The first turn of a session in `project`, or `None` when it starts without a prompt.
///
/// A session started from an item needs a prompt, which follows the item's context.
///
/// # Errors
/// Answers `400` for a session started from an item without a prompt, and as
/// [`item_context`] does.
pub async fn build(
    connection: &mut AsyncPgConnection,
    project: &Project,
    action_item_id: Option<Uuid>,
    prompt: Option<String>,
    now: DateTime<Utc>,
) -> Result<Option<String>, ApiError> {
    match (action_item_id, prompt) {
        (None, prompt) => Ok(prompt),
        (Some(_action_item_id), None) => Err(ApiError::BadRequest(
            "a session started from an action item needs a prompt".to_owned(),
        )),
        (Some(action_item_id), Some(prompt)) => {
            let turn = item_context(connection, action_item_id, project, &prompt, now).await?;
            Ok(Some(turn))
        }
    }
}

/// The first turn for a session in `project` started from the item `action_item_id`: the
/// item's context, then `request`.
///
/// A session started from an item belongs to one of the item's projects; an item in no
/// project lets the session belong to any. See `docs/action-items.md`.
///
/// # Errors
/// Answers `404` for an unknown item, `409` for a deleted one, and `400` when `project` is
/// not one of the item's projects.
async fn item_context(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
    project: &Project,
    request: &str,
    now: DateTime<Utc>,
) -> Result<String, ApiError> {
    let item = action_item::find(connection, action_item_id).await?;
    if item.deleted_at.is_some() {
        return Err(ApiError::Conflict(
            "the action item is deleted; restore it before starting a session from it",
        ));
    }
    let memberships = action_item::memberships(connection, &[item.id]).await?;
    let project_ids = memberships.project_ids(item.id);
    if !project_ids.is_empty() && !project_ids.contains(&project.id) {
        return Err(ApiError::BadRequest(
            "a session started from an action item belongs to one of the item's projects"
                .to_owned(),
        ));
    }

    let comments = action_item_comment::list(connection, item.id).await?;
    let initiatives =
        initiative::find_live(connection, &memberships.initiative_ids(item.id)).await?;
    let initiative_ids: Vec<Uuid> = initiatives.iter().map(|found| found.id).collect();
    let spans = initiative::memberships(connection, &initiative_ids).await?;
    let initiatives: Vec<_> = initiatives
        .into_iter()
        .map(|found| {
            let spans = spans.get(&found.id).map_or(&[][..], Vec::as_slice);
            let progress = progress::at(spans, now);
            (found, progress)
        })
        .collect();

    Ok(session_context::first_turn(
        ItemContext {
            item: &item,
            project,
            initiatives: &initiatives,
            comments: &comments,
        },
        request,
    ))
}
