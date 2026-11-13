// Copyright © 2026 Jalapeno Labs

//! A meeting's tasks (`docs/meetings.md`, Tasks): `GET /api/v1/meetings/{id}/tasks` lists every
//! task heard in it, in the order they were said, each with whose it is, the items made of it,
//! the changes waiting for a person that would make one, and why its Project Manager made none,
//! for the bar along the meeting's top. `GET /api/v1/meetings/task-items/{itemId}` answers the task
//! an action item was made of, for the item's page to open its words.
//!
//! Both answer whoever may read the meeting, as its other reading routes do
//! (`crate::meetings::access`); `404` for a meeting they may not read. A task's share is the
//! meeting's person's: their own tasks name them, so a reader other than its person sees whose they
//! are.

use std::collections::HashMap;

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use diesel::OptionalExtension;
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::open;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::access::Need;
use crate::meetings::excerpts::MeetingText;
use crate::meetings::tasks::{Share, share_of};
use crate::models::action_item::ActionItemState;
use crate::models::meeting::{self, Reader};
use crate::models::meeting_mark;
use crate::models::meeting_task::{self, TaskReading};
use crate::models::meeting_word;
use crate::models::user;
use crate::state::AppState;

/// Whose a task is, as the bar groups it. Its own is the meeting's person's, by id and name.
#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
enum ShareResponse {
    Project { project_id: Uuid },
    Own { user_id: Uuid, name: String },
    Coworker { user_id: Uuid, name: String },
    WaitingOn { name: String },
    Other,
}

/// An item made of a task.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskItemResponse {
    id: Uuid,
    title: String,
    state: ActionItemState,
}

/// Why a task's Project Manager made no item of it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeclinedResponse {
    reason: String,
    at: DateTime<Utc>,
}

/// A task as the meeting's page shows it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskResponse {
    /// Its mark, which is its id.
    id: Uuid,
    title: String,
    start_word_id: Uuid,
    end_word_id: Uuid,
    /// When its words start on the meeting's clock.
    start_ms: i64,
    share: ShareResponse,
    /// The items made of it that stand.
    items: Vec<TaskItemResponse>,
    /// How many items made of it were deleted since.
    deleted_items: usize,
    /// The changesets waiting for a person that would make an item of it.
    proposals: Vec<Uuid>,
    declined: Option<DeclinedResponse>,
}

pub async fn list(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let (found, _access) = open(&mut connection, id, &current, Need::Read).await?;
    let tasks = meeting_task::standing_of(&mut connection, found.id).await?;
    let task_ids: Vec<Uuid> = tasks.keys().copied().collect();
    let mut items = meeting_task::items_of(&mut connection, &task_ids).await?;
    let mut proposals = meeting_task::pending_proposals_of(&mut connection, &task_ids).await?;
    let names = user::names(&mut connection).await?;
    let words = meeting_word::standing(&mut connection, found.id).await?;
    let marks = meeting_mark::standing(&mut connection, found.id).await?;
    let text = MeetingText::new(words, &marks);

    let mut ordered: Vec<(usize, TaskResponse)> = Vec::with_capacity(tasks.len());
    for mark in &marks {
        let Some(task) = tasks.get(&mark.id) else {
            continue;
        };
        let Some(places) = text.places_of(mark.range()) else {
            // Marks move with every edit, so a standing mark is always over standing words.
            tracing::event!(
                name: "meetings.task.unplaced",
                tracing::Level::WARN,
                meeting_task.id = %mark.id,
                "a standing task is not over standing words; the bar leaves it out",
            );
            continue;
        };
        let start = *places.start();
        let made = items.remove(&task.mark_id).unwrap_or_default();
        ordered.push((
            start,
            TaskResponse {
                id: task.mark_id,
                title: task.title.clone(),
                start_word_id: mark.start_word_id,
                end_word_id: mark.end_word_id,
                start_ms: text.words()[start].start_ms,
                share: share_response(&task.reading(), found.created_by, &names),
                items: made
                    .live
                    .into_iter()
                    .map(|item| TaskItemResponse {
                        id: item.id,
                        title: item.title,
                        state: item.state,
                    })
                    .collect(),
                deleted_items: made.deleted,
                proposals: proposals.remove(&task.mark_id).unwrap_or_default(),
                declined: task
                    .declined_reason
                    .clone()
                    .zip(task.declined_at)
                    .map(|(reason, at)| DeclinedResponse { reason, at }),
            },
        ));
    }
    ordered.sort_by_key(|(start, task)| (*start, task.id));
    let tasks: Vec<TaskResponse> = ordered.into_iter().map(|(_start, task)| task).collect();
    Ok(Json(json!({ "tasks": tasks })))
}

/// Whose `reading` is in a meeting whose person is `owner`, as the bar names it: the person and a
/// coworker by their names.
fn share_response(
    reading: &TaskReading,
    owner: Uuid,
    names: &HashMap<Uuid, String>,
) -> ShareResponse {
    let name_of = |user_id: Uuid| names.get(&user_id).cloned().unwrap_or_default();
    match share_of(reading, owner) {
        Share::Project(project_id) => ShareResponse::Project { project_id },
        Share::Own => ShareResponse::Own {
            user_id: owner,
            name: name_of(owner),
        },
        Share::Coworker(user_id) => ShareResponse::Coworker {
            user_id,
            name: name_of(user_id),
        },
        Share::WaitingOn(name) => ShareResponse::WaitingOn { name },
        Share::Other => ShareResponse::Other,
    }
}

/// `GET /api/v1/meetings/task-items/{itemId}`: the task an action item was made of, as its meeting
/// and its id, for the item's page to open its words. Every item's page asks, so an item no task of
/// a meeting the signed-in person may read made answers `{ "task": null }`.
pub async fn of_item(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(item_id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let Some((meeting_id, task_id)) = meeting_task::task_of_item(&mut connection, item_id).await?
    else {
        return Ok(Json(json!({ "task": null })));
    };
    let readable = meeting::find_readable(&mut connection, meeting_id, Reader::of(&current.0))
        .await
        .optional()?;
    let task = readable.map(
        |found| json!({ "meetingId": found.id, "meetingTitle": found.title, "taskId": task_id }),
    );
    Ok(Json(json!({ "task": task })))
}
