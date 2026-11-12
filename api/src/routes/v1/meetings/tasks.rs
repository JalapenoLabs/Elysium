// Copyright © 2026 Jalapeno Labs

//! A meeting's tasks (`docs/meetings.md`, Tasks): `GET /api/v1/meetings/{id}/tasks` lists every
//! task heard in it, in the order they were said, each with whose it is, the items made of it,
//! the changes waiting for a person that would make one, and why its Project Manager made none,
//! for the bar along the meeting's top. `GET /api/v1/meetings/task-items/{itemId}` answers the task
//! an action item was made of, for the item's page to open its words.
//!
//! Both answer the meeting's owner alone, as the meeting's other routes do; `404` for a meeting
//! that is not theirs.

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

use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::excerpts::MeetingText;
use crate::meetings::tasks::{Share, share_of};
use crate::models::action_item::ActionItemState;
use crate::models::meeting;
use crate::models::meeting_mark;
use crate::models::meeting_task;
use crate::models::meeting_word;
use crate::models::user;
use crate::state::AppState;

/// Whose a task is, as the bar groups it.
#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
enum ShareResponse {
    Project { project_id: Uuid },
    Own,
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
    items: Vec<TaskItemResponse>,
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
    let found = meeting::find_owned(&mut connection, id, current.id()).await?;
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
        ordered.push((
            start,
            TaskResponse {
                id: task.mark_id,
                title: task.title.clone(),
                start_word_id: mark.start_word_id,
                end_word_id: mark.end_word_id,
                start_ms: text.words()[start].start_ms,
                share: share_response(share_of(&task.reading(), found.created_by), &names),
                items: items
                    .remove(&task.mark_id)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|item| TaskItemResponse {
                        id: item.id,
                        title: item.title,
                        state: item.state,
                    })
                    .collect(),
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

/// `share` as the bar names it: a coworker by their name.
fn share_response(share: Share, names: &HashMap<Uuid, String>) -> ShareResponse {
    match share {
        Share::Project(project_id) => ShareResponse::Project { project_id },
        Share::Own => ShareResponse::Own,
        Share::Coworker(user_id) => ShareResponse::Coworker {
            user_id,
            name: names.get(&user_id).cloned().unwrap_or_default(),
        },
        Share::WaitingOn(name) => ShareResponse::WaitingOn { name },
        Share::Other => ShareResponse::Other,
    }
}

/// `GET /api/v1/meetings/task-items/{itemId}`: the task an action item was made of, as its meeting
/// and its id, for the item's page to open its words. Every item's page asks, so an item no task of
/// the signed-in person's meetings made answers `{ "task": null }`.
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
    let owned = meeting::find_owned(&mut connection, meeting_id, current.id())
        .await
        .optional()?;
    let task = owned.map(
        |found| json!({ "meetingId": found.id, "meetingTitle": found.title, "taskId": task_id }),
    );
    Ok(Json(json!({ "task": task })))
}
