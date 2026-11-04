// Copyright © 2026 Jalapeno Labs

//! Submitting and withdrawing a Project Manager's work batches: the rules a batch is held to,
//! and the one place it is stored, put on the timeline, and announced.
//!
//! Nothing a submission names is taken on trust: its repositories follow the rules a coding
//! session's do, a scheduled time is in the future and within a year, and every record and
//! action item it names as its provenance is on its project. The rules are [`check`], a pure
//! function tested on its own; [`submit`] adds the checks that read the database.

use std::collections::HashSet;

use chrono::{DateTime, TimeDelta, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::timeline;
use crate::database::schema::{action_item_projects, action_items};
use crate::models::timeline_record;
use crate::models::user::PROJECT_MANAGER_USER_ID;
use crate::models::work_batch::{self, NewWorkBatch, Priority, Repository, WorkBatch};
use crate::realtime::{EventBus, ServerEvent};
use crate::routes::v1::coding_sessions::validate_repository_url;

/// The longest title, matching `work_batches.title`.
const TITLE_MAX_CHARACTERS: usize = 200;

/// The longest goal and cancellation reason, matching `work_batches.goal` and `result`.
const TEXT_MAX_CHARACTERS: usize = 20_000;

/// The most repositories one batch needs, as a coding session clones at most.
const REPOSITORIES_MAX: usize = 16;

/// The most records and action items one batch names as its provenance. A batch is one piece of
/// work; more than this is several.
const PROVENANCE_MAX: usize = 50;

/// How far ahead a batch may be scheduled. Further than a year is a plan, not a batch.
const SCHEDULE_HORIZON: TimeDelta = TimeDelta::days(365);

/// A batch as a Project Manager submits it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Submission {
    pub title: String,
    pub goal: String,
    #[serde(default)]
    pub repositories: Vec<Repository>,
    pub priority: Priority,
    /// When a scheduled batch is due; given exactly for `scheduled`.
    #[serde(default)]
    pub scheduled_for: Option<DateTime<Utc>>,
    /// The timeline records that called for it.
    #[serde(default)]
    pub record_ids: Vec<Uuid>,
    /// The action items it works on.
    #[serde(default)]
    pub action_item_ids: Vec<Uuid>,
}

/// Why a batch was refused, in words the Project Manager can act on.
#[derive(Debug, thiserror::Error)]
pub enum BatchError {
    #[error("{0}")]
    Refused(String),
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// The rules every submission is held to at `now`, before anything is read. Answers what to
/// change.
///
/// # Errors
/// Names the first rule `submission` breaks.
pub fn check(submission: &Submission, now: DateTime<Utc>) -> Result<(), String> {
    let title = submission.title.trim();
    if title.is_empty() || title.chars().count() > TITLE_MAX_CHARACTERS {
        return Err(format!(
            "title must hold text, at most {TITLE_MAX_CHARACTERS} characters"
        ));
    }
    let goal = submission.goal.trim();
    if goal.is_empty() || goal.chars().count() > TEXT_MAX_CHARACTERS {
        return Err(format!(
            "goal must hold text, at most {TEXT_MAX_CHARACTERS} characters"
        ));
    }

    if submission.repositories.len() > REPOSITORIES_MAX {
        return Err(format!(
            "a batch needs at most {REPOSITORIES_MAX} repositories; split the work"
        ));
    }
    let mut urls = HashSet::new();
    for repository in &submission.repositories {
        if let Err(refusal) = validate_repository_url(&repository.url) {
            let reason = refusal
                .message
                .unwrap_or_else(|| "is not a repository URL".into());
            return Err(format!("repository {}: {reason}", repository.url));
        }
        if !urls.insert(repository.url.trim_end_matches('/').to_lowercase()) {
            return Err(format!("repository {} is named twice", repository.url));
        }
        let blank_branch = repository
            .base_branch
            .as_deref()
            .is_some_and(|branch| branch.trim().is_empty() || branch.chars().count() > 255);
        if blank_branch {
            return Err(format!(
                "repository {}: baseBranch must hold text, at most 255 characters, or be left out",
                repository.url
            ));
        }
    }

    match (submission.priority, submission.scheduled_for) {
        (Priority::Scheduled, None) => {
            return Err(
                "a scheduled batch needs scheduledFor, the time it is due, with its offset"
                    .to_owned(),
            );
        }
        (Priority::Scheduled, Some(at)) if at <= now => {
            return Err(
                "scheduledFor is not in the future; submit it as asap or normal instead".to_owned(),
            );
        }
        (Priority::Scheduled, Some(at)) if at > now + SCHEDULE_HORIZON => {
            return Err("scheduledFor is more than a year away".to_owned());
        }
        (Priority::Asap | Priority::Normal, Some(_)) => {
            return Err("scheduledFor is only for a scheduled batch".to_owned());
        }
        _ => {}
    }

    for (name, ids) in [
        ("recordIds", &submission.record_ids),
        ("actionItemIds", &submission.action_item_ids),
    ] {
        if ids.len() > PROVENANCE_MAX {
            return Err(format!("{name} names at most {PROVENANCE_MAX} ids"));
        }
        if ids.iter().collect::<HashSet<_>>().len() != ids.len() {
            return Err(format!("{name} names an id twice"));
        }
    }
    Ok(())
}

/// Submits `submission` for `project_id` from its Project Manager's turn `turn_id`: stores it,
/// puts it on the project's timeline, and announces both.
///
/// # Errors
/// [`BatchError::Refused`] naming what to change, when [`check`] refuses it or it names a record
/// or action item that is not on the project; any database error.
pub async fn submit(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    project_id: Uuid,
    turn_id: Uuid,
    submission: Submission,
) -> Result<WorkBatch, BatchError> {
    check(&submission, Utc::now()).map_err(BatchError::Refused)?;
    refuse_unknown_provenance(connection, project_id, &submission).await?;

    let repositories: Vec<Repository> = submission
        .repositories
        .into_iter()
        .map(|repository| Repository {
            url: repository.url.trim().to_owned(),
            base_branch: repository
                .base_branch
                .map(|branch| branch.trim().to_owned()),
        })
        .collect();
    let new_batch = NewWorkBatch {
        id: Uuid::now_v7(),
        project_id,
        title: submission.title.trim().to_owned(),
        goal: submission.goal.trim().to_owned(),
        repositories: json!(repositories),
        priority: submission.priority,
        scheduled_for: submission.scheduled_for,
        turn_id: Some(turn_id),
        source_record_ids: json!(submission.record_ids),
        action_item_ids: json!(submission.action_item_ids),
        created_by: PROJECT_MANAGER_USER_ID,
    };
    let (batch, record) = connection
        .transaction(async |connection| {
            let mut batch = work_batch::submit(connection, new_batch).await?;
            let record = timeline::record_batch(connection, &batch).await?;
            work_batch::set_timeline_record(connection, batch.id, record.id).await?;
            batch.timeline_record_id = Some(record.id);
            diesel::QueryResult::Ok((batch, record))
        })
        .await?;

    events.publish(&ServerEvent::TimelineRecordCreated {
        id: record.id,
        source_kind: record.source_kind,
        created_by: record.created_by,
        action_item_id: None,
        conversation_id: None,
    });
    events.publish(&ServerEvent::ProjectManagerChanged { project_id });
    Ok(batch)
}

/// Withdraws `project_id`'s queued batch `batch_id`, saying `reason`, and announces it.
///
/// # Errors
/// [`BatchError::Refused`] when the project has no such batch waiting; any database error.
pub async fn cancel(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    project_id: Uuid,
    batch_id: Uuid,
    reason: &str,
) -> Result<WorkBatch, BatchError> {
    let reason = reason.trim();
    if reason.is_empty() || reason.chars().count() > TEXT_MAX_CHARACTERS {
        return Err(BatchError::Refused(format!(
            "reason must hold text, at most {TEXT_MAX_CHARACTERS} characters"
        )));
    }
    let cancelled = work_batch::cancel(connection, project_id, batch_id, reason)
        .await?
        .ok_or_else(|| {
            BatchError::Refused(format!(
                "batch {batch_id} is not a queued batch of this project; a worker may have taken \
                 it already. Call manager_batches for where each stands"
            ))
        })?;
    events.publish(&ServerEvent::ProjectManagerChanged { project_id });
    Ok(cancelled)
}

/// Refuses a submission naming a record not attributed to its project, or an action item that is
/// deleted or not on it.
async fn refuse_unknown_provenance(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    submission: &Submission,
) -> Result<(), BatchError> {
    if !submission.record_ids.is_empty() {
        let found: HashSet<Uuid> =
            timeline_record::attributed_among(connection, project_id, &submission.record_ids)
                .await?
                .into_iter()
                .map(|(record, _attribution)| record.id)
                .collect();
        if let Some(missing) = submission.record_ids.iter().find(|id| !found.contains(id)) {
            return Err(BatchError::Refused(format!(
                "record {missing} is not on this project's timeline; call manager_timeline for \
                 the ones that are"
            )));
        }
    }
    if !submission.action_item_ids.is_empty() {
        let found: HashSet<Uuid> = action_item_projects::table
            .inner_join(action_items::table)
            .filter(action_item_projects::project_id.eq(project_id))
            .filter(action_item_projects::action_item_id.eq_any(&submission.action_item_ids))
            .filter(action_items::deleted_at.is_null())
            .select(action_item_projects::action_item_id)
            .load::<Uuid>(connection)
            .await?
            .into_iter()
            .collect();
        if let Some(missing) = submission
            .action_item_ids
            .iter()
            .find(|id| !found.contains(id))
        {
            return Err(BatchError::Refused(format!(
                "action item {missing} is not in this project or was deleted; call work_items for \
                 the ones that are"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn submission() -> Submission {
        Submission {
            title: "Annual pricing".to_owned(),
            goal: "Show annual pricing on the pricing page, as Ada asked.".to_owned(),
            repositories: vec![Repository {
                url: "https://github.com/JalapenoLabs/Elysium.git".to_owned(),
                base_branch: None,
            }],
            priority: Priority::Asap,
            scheduled_for: None,
            record_ids: vec![Uuid::now_v7()],
            action_item_ids: Vec::new(),
        }
    }

    #[test]
    fn a_complete_batch_passes() {
        check(&submission(), Utc::now()).expect("valid");
    }

    #[test]
    fn a_batch_needs_a_title_and_a_goal() {
        let mut untitled = submission();
        untitled.title = "   ".to_owned();
        assert!(check(&untitled, Utc::now()).is_err_and(|reason| reason.contains("title")));
        let mut aimless = submission();
        aimless.goal = String::new();
        assert!(check(&aimless, Utc::now()).is_err_and(|reason| reason.contains("goal")));
    }

    #[test]
    fn repositories_follow_a_sessions_rules_and_are_named_once() {
        let mut ssh_elsewhere = submission();
        ssh_elsewhere.repositories[0].url = "git@gitlab.com:org/repo.git".to_owned();
        assert!(check(&ssh_elsewhere, Utc::now()).is_err_and(|reason| reason.contains("SSH")));

        let mut twice = submission();
        twice.repositories.push(Repository {
            url: "https://github.com/jalapenolabs/elysium.git/".to_owned(),
            base_branch: Some("main".to_owned()),
        });
        assert!(check(&twice, Utc::now()).is_err_and(|reason| reason.contains("twice")));

        let mut blank_branch = submission();
        blank_branch.repositories[0].base_branch = Some(" ".to_owned());
        assert!(
            check(&blank_branch, Utc::now()).is_err_and(|reason| reason.contains("baseBranch"))
        );
    }

    #[test]
    fn a_scheduled_time_is_given_exactly_for_a_scheduled_batch_in_the_future() {
        let now = Utc::now();
        let mut unscheduled = submission();
        unscheduled.priority = Priority::Scheduled;
        assert!(
            check(&unscheduled, now).is_err_and(|reason| reason.contains("needs scheduledFor"))
        );

        let mut past = unscheduled.clone();
        past.scheduled_for = Some(now - TimeDelta::minutes(1));
        assert!(check(&past, now).is_err_and(|reason| reason.contains("not in the future")));

        let mut far = unscheduled.clone();
        far.scheduled_for = Some(now + TimeDelta::days(400));
        assert!(check(&far, now).is_err_and(|reason| reason.contains("a year")));

        let mut tomorrow = unscheduled;
        tomorrow.scheduled_for = Some(now + TimeDelta::days(1));
        check(&tomorrow, now).expect("a scheduled batch for tomorrow");

        let mut asap_with_time = submission();
        asap_with_time.scheduled_for = Some(now + TimeDelta::days(1));
        assert!(check(&asap_with_time, now).is_err_and(|reason| reason.contains("only for")));
    }

    #[test]
    fn provenance_names_each_id_once() {
        let mut twice = submission();
        let id = Uuid::now_v7();
        twice.action_item_ids = vec![id, id];
        assert!(check(&twice, Utc::now()).is_err_and(|reason| reason.contains("actionItemIds")));
    }

    #[test]
    fn a_submission_reads_as_the_tool_sends_it() {
        let read: Submission = serde_json::from_value(json!({
            "title": "Nightly report",
            "goal": "Send the nightly report.",
            "priority": "scheduled",
            "scheduledFor": "2026-11-05T09:00:00Z",
            "repositories": [{ "url": "https://github.com/org/repo", "baseBranch": "develop" }],
        }))
        .expect("reads");
        assert_eq!(read.priority, Priority::Scheduled);
        assert_eq!(read.repositories[0].base_branch.as_deref(), Some("develop"));
        assert!(read.record_ids.is_empty());

        serde_json::from_value::<Submission>(json!({
            "title": "x", "goal": "y", "priority": "urgent",
        }))
        .expect_err("asap, normal, or scheduled");
        serde_json::from_value::<Submission>(json!({
            "title": "x", "goal": "y", "priority": "asap", "worker": "me",
        }))
        .expect_err("nothing else");
    }
}
