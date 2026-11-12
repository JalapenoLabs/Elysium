// Copyright © 2026 Jalapeno Labs

//! The tasks heard in meetings for a Project Manager's project: what a turn is told of each, and
//! its answer when it makes no item of one. See `docs/engine.md`, Project Managers, and
//! `docs/meetings.md`, Tasks.
//!
//! A task reaches its project's Project Manager as a `task` wake (`crate::meetings::tasks`). A turn
//! is told each one still standing and still the project's ([`load`]): what is to be done, who
//! owes it, the meeting it was heard in, its words, and the excerpts on the project its words lie
//! in, never the meeting whole, and what came of it before when a meeting read again changed it.
//! It answers each with `manager_make_changes`, a create naming the task, or with
//! `manager_decline_task`, which records why it made none ([`decline`]), so the meeting's page
//! says it.

use std::collections::HashMap;
use std::ops::RangeInclusive;

use chrono::Utc;
use diesel_async::AsyncPgConnection;
use uuid::Uuid;

use super::prompt::{PromptTask, PromptTaskExcerpt};
use crate::meetings::excerpts::MeetingText;
use crate::meetings::segmentation::clock;
use crate::models::meeting_mark::{self, Mark, MarkKind};
use crate::models::meeting_task::{self, DECLINE_REASON_MAX_CHARACTERS, MeetingTask, OwedBy};
use crate::models::{meeting, meeting_word, timeline_record};

/// The tasks among `task_ids` a turn of `project_id`'s Project Manager acts on, oldest first: each
/// still standing and still the project's, as [`PromptTask`]s, its people named from `names`. A
/// task read onto another project since, or heard no longer, is left out: it is not this project's
/// to act on.
///
/// # Errors
/// Propagates any database error.
pub async fn load(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    task_ids: &[Uuid],
    names: &HashMap<Uuid, String>,
) -> diesel::QueryResult<Vec<PromptTask>> {
    let tasks = meeting_task::standing_for_project(connection, project_id, task_ids).await?;
    if tasks.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<Uuid> = tasks.iter().map(|task| task.mark_id).collect();
    let mut items = meeting_task::items_of(connection, &ids).await?;
    let mut proposed = meeting_task::pending_proposals_of(connection, &ids).await?;

    let mut by_meeting: HashMap<Uuid, Vec<MeetingTask>> = HashMap::new();
    for task in tasks {
        by_meeting.entry(task.meeting_id).or_default().push(task);
    }
    let mut prompted = Vec::with_capacity(ids.len());
    for (meeting_id, tasks) in by_meeting {
        let found = meeting::find(connection, meeting_id).await?;
        let words = meeting_word::standing(connection, meeting_id).await?;
        let marks = meeting_mark::standing(connection, meeting_id).await?;
        let text = MeetingText::new(words, &marks);
        let excerpts = excerpts_on(connection, project_id, &marks, &text).await?;
        let timed = text.is_timed();
        for task in tasks {
            let Some(mark) = marks.iter().find(|mark| mark.id == task.mark_id) else {
                continue;
            };
            let Some(places) = text.places_of(mark.range()) else {
                // Marks move with every edit, so a standing mark is always over standing words.
                tracing::event!(
                    name: "project_managers.task.unplaced",
                    tracing::Level::WARN,
                    meeting_task.id = %task.mark_id,
                    "a standing task is not over standing words; its turn leaves it out",
                );
                continue;
            };
            let reading = text.read(places.clone());
            let around: Vec<PromptTaskExcerpt> = excerpts
                .iter()
                .filter(|(excerpt_places, _excerpt)| {
                    excerpt_places.start() <= places.end() && places.start() <= excerpt_places.end()
                })
                .map(|(_places, excerpt)| excerpt.clone())
                .collect();
            prompted.push(PromptTask {
                id: task.mark_id,
                owed_by: owed_by_words(&task.owed_by(), found.created_by, names),
                meeting_id,
                meeting_title: found.public_title().map(str::to_owned),
                heard_at: timed.then(|| clock(reading.start_ms)),
                words: reading.body,
                excerpts: around,
                items: items
                    .remove(&task.mark_id)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|item| (item.id, item.title, item.state))
                    .collect(),
                proposed: proposed.remove(&task.mark_id).unwrap_or_default(),
                declined: task.declined_reason.clone(),
                title: task.title,
            });
        }
    }
    prompted.sort_by_key(|task| task.id);
    Ok(prompted)
}

/// The excerpts among a meeting's standing `marks` whose records are on `project_id`, each by the
/// places of its words in `text`, the meeting's transcript as it reads.
async fn excerpts_on(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    marks: &[Mark],
    text: &MeetingText,
) -> diesel::QueryResult<Vec<(RangeInclusive<usize>, PromptTaskExcerpt)>> {
    let mark_ids: Vec<Uuid> = marks
        .iter()
        .filter(|mark| mark.kind() == Some(MarkKind::Excerpt))
        .map(|mark| mark.id)
        .collect();
    let records = timeline_record::of_excerpts(connection, &mark_ids).await?;
    let record_ids: Vec<Uuid> = records.iter().map(|record| record.id).collect();
    let on_project = timeline_record::attributed_among(connection, project_id, &record_ids).await?;
    let mut excerpts = Vec::with_capacity(on_project.len());
    for (record, _attribution) in on_project {
        let places = marks
            .iter()
            .find(|mark| Some(mark.id) == record.excerpt_mark_id)
            .and_then(|mark| text.places_of(mark.range()));
        let Some(places) = places else {
            continue;
        };
        excerpts.push((
            places,
            PromptTaskExcerpt {
                record_id: record.id,
                summary: record.metadata["summary"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                title: record.title,
            },
        ));
    }
    Ok(excerpts)
}

/// Who owes a task, as its turn reads it.
fn owed_by_words(owed_by: &OwedBy, owner: Uuid, names: &HashMap<Uuid, String>) -> String {
    match owed_by {
        OwedBy::Person { user_id } => {
            let name = names
                .get(user_id)
                .cloned()
                .unwrap_or_else(|| "someone".to_owned());
            if *user_id == owner {
                return format!("{name}, the person whose meeting it was");
            }
            format!("{name}, a person on Elysium")
        }
        OwedBy::Named { name } => format!("{name}, who is not on Elysium"),
        OwedBy::Nobody => "nobody named in the meeting".to_owned(),
    }
}

/// Why a Project Manager's answer about a task was refused.
#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    /// No standing task of a meeting is for this project by that id.
    #[error(
        "task {0} is not one of this project's tasks; name one your prompt lists under Tasks heard \
         in meetings"
    )]
    NotThisProjects(Uuid),
    #[error("the reason must be 1 to {DECLINE_REASON_MAX_CHARACTERS} characters")]
    Reason,
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// Whether `task_id` is a standing task for `project_id`, which its Project Manager may act on.
///
/// # Errors
/// [`TaskError::NotThisProjects`] when it is not; any database error.
pub async fn require_projects(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    task_id: Uuid,
) -> Result<MeetingTask, TaskError> {
    meeting_task::standing_for_project(connection, project_id, &[task_id])
        .await?
        .into_iter()
        .next()
        .ok_or(TaskError::NotThisProjects(task_id))
}

/// Records why `project_id`'s Project Manager made no item of task `task_id`: `reason`, on one
/// line and trimmed, which the meeting's bar shows with the task.
///
/// # Errors
/// [`TaskError::NotThisProjects`] for a task that is not the project's, [`TaskError::Reason`] for
/// an empty or too long reason; any database error.
pub async fn decline(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    task_id: Uuid,
    reason: &str,
) -> Result<MeetingTask, TaskError> {
    let reason = reason.split_whitespace().collect::<Vec<_>>().join(" ");
    if reason.is_empty() || reason.chars().count() > DECLINE_REASON_MAX_CHARACTERS {
        return Err(TaskError::Reason);
    }
    require_projects(connection, project_id, task_id).await?;
    Ok(meeting_task::decline(connection, task_id, &reason, Utc::now()).await?)
}
