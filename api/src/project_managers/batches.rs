// Copyright © 2026 Jalapeno Labs

//! Submitting and withdrawing a Project Manager's work batches: the rules a batch is held to,
//! and the one place it is stored, put on the timeline, and announced.
//!
//! Nothing a submission names is taken on trust: its repositories follow the rules a coding
//! session's do, a scheduled time is in the future and within a year, and every record and
//! action item it names as its provenance is on its project. The rules are [`check`], a pure
//! function tested on its own; [`submit`] adds the checks that read the database. A person's
//! edits to a proposed batch are held to the same rules ([`check_edits`]).
//!
//! Every batch serves a goal in its project's brief (`crate::models::project_brief`), named by
//! the goal's heading, and is refused when the brief has no such goal: work outside the brief is
//! the person's to decide, so the refusal says to ask them rather than to submit it some other
//! way. The batch stores the goal as the brief spells it ([`goal_served`]).
//!
//! A submitted batch is proposed: it waits for a person to approve it before the pool may give
//! it a slot (`crate::models::work_batch::approval`).

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::timeline;
use crate::database::schema::{action_item_projects, action_items};
use crate::models::user::PROJECT_MANAGER_USER_ID;
use crate::models::work_batch::approval::Edits;
use crate::models::work_batch::{self, ChangeError, NewWorkBatch, Priority, Repository, WorkBatch};
use crate::models::{project_brief, timeline_record};
use crate::realtime::{EventBus, ServerEvent};
use crate::routes::v1::coding_sessions::validate_repository_url;
use crate::worker_pool::actions;

/// The longest title, matching `work_batches.title`.
const TITLE_MAX_CHARACTERS: usize = 200;

/// The longest goal, matching `work_batches.goal`.
const TEXT_MAX_CHARACTERS: usize = 20_000;

/// The most repositories one batch needs, as a coding session clones at most.
const REPOSITORIES_MAX: usize = 16;

/// The most records and action items one batch names as its provenance. A batch is one piece of
/// work; more than this is several.
const PROVENANCE_MAX: usize = 50;

/// The longest goal a batch names as the one it serves, matching `work_batches.serves`: a heading,
/// as long as a page's title may be.
const SERVES_MAX_CHARACTERS: usize = 300;

/// A batch as a Project Manager submits it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Submission {
    pub title: String,
    pub goal: String,
    /// The goal in its project's brief it serves, by the goal's heading.
    pub serves: String,
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
    check_title(&submission.title)?;
    check_goal(&submission.goal)?;
    let serves = submission.serves.trim();
    if serves.is_empty() || serves.chars().count() > SERVES_MAX_CHARACTERS {
        return Err(format!(
            "serves must name the goal in the project's brief this batch serves, by its \
             heading, at most {SERVES_MAX_CHARACTERS} characters"
        ));
    }
    check_repositories(&submission.repositories)?;
    work_batch::check_schedule(submission.priority, submission.scheduled_for, now)?;

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

/// `edits` a person makes to a proposed batch at `now`, tidied as a submission's fields are and
/// held to the same rules: each field given is checked alone, since each replaces only itself.
///
/// # Errors
/// Names the first rule an edit breaks.
pub fn check_edits(edits: Edits, now: DateTime<Utc>) -> Result<Edits, String> {
    let title = edits.title.map(|title| title.trim().to_owned());
    if let Some(title) = &title {
        check_title(title)?;
    }
    let goal = edits.goal.map(|goal| goal.trim().to_owned());
    if let Some(goal) = &goal {
        check_goal(goal)?;
    }
    let repositories = edits.repositories.map(tidy_repositories);
    if let Some(repositories) = &repositories {
        check_repositories(repositories)?;
    }
    if let Some((priority, scheduled_for)) = edits.priority {
        work_batch::check_schedule(priority, scheduled_for, now)?;
    }
    Ok(Edits {
        title,
        goal,
        repositories,
        priority: edits.priority,
    })
}

fn check_title(title: &str) -> Result<(), String> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > TITLE_MAX_CHARACTERS {
        return Err(format!(
            "title must hold text, at most {TITLE_MAX_CHARACTERS} characters"
        ));
    }
    Ok(())
}

fn check_goal(goal: &str) -> Result<(), String> {
    let goal = goal.trim();
    if goal.is_empty() || goal.chars().count() > TEXT_MAX_CHARACTERS {
        return Err(format!(
            "goal must hold text, at most {TEXT_MAX_CHARACTERS} characters"
        ));
    }
    Ok(())
}

/// The rules a batch's repositories follow: a coding session's, each named once, at most
/// [`REPOSITORIES_MAX`].
fn check_repositories(repositories: &[Repository]) -> Result<(), String> {
    if repositories.len() > REPOSITORIES_MAX {
        return Err(format!(
            "a batch needs at most {REPOSITORIES_MAX} repositories; split the work"
        ));
    }
    let mut urls = HashSet::new();
    for repository in repositories {
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
    Ok(())
}

/// `repositories` as they are stored: each URL and base branch trimmed.
fn tidy_repositories(repositories: Vec<Repository>) -> Vec<Repository> {
    repositories
        .into_iter()
        .map(|repository| Repository {
            url: repository.url.trim().to_owned(),
            base_branch: repository
                .base_branch
                .map(|branch| branch.trim().to_owned()),
        })
        .collect()
}

/// Submits `submission` for `project_id` from its Project Manager's turn `turn_id`: stores it,
/// proposed, puts it on the project's timeline, raises or updates the project's approval item,
/// and announces all three.
///
/// # Errors
/// [`BatchError::Refused`] naming what to change, when [`check`] refuses it, it serves no goal in
/// the project's brief, or it names a record or action item that is not on the project; any
/// database error.
pub async fn submit(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    project_id: Uuid,
    turn_id: Uuid,
    submission: Submission,
) -> Result<WorkBatch, BatchError> {
    check(&submission, Utc::now()).map_err(BatchError::Refused)?;
    let serves = goal_served(connection, project_id, &submission.serves).await?;
    refuse_unknown_provenance(connection, project_id, &submission).await?;

    let repositories = tidy_repositories(submission.repositories);
    let new_batch = NewWorkBatch {
        id: Uuid::now_v7(),
        project_id,
        title: submission.title.trim().to_owned(),
        goal: submission.goal.trim().to_owned(),
        serves: Some(serves),
        repositories: json!(repositories),
        priority: submission.priority,
        scheduled_for: submission.scheduled_for,
        turn_id: Some(turn_id),
        source_record_ids: json!(submission.record_ids),
        action_item_ids: json!(submission.action_item_ids),
        created_by: PROJECT_MANAGER_USER_ID,
    };
    let (written, record) = connection
        .transaction(async |connection| {
            let mut written = work_batch::submit(connection, new_batch).await?;
            let record = timeline::record_batch(connection, &written.batch).await?;
            work_batch::set_timeline_record(connection, written.batch.id, record.id).await?;
            written.batch.timeline_record_id = Some(record.id);
            diesel::QueryResult::Ok((written, record))
        })
        .await?;

    events.publish(&ServerEvent::TimelineRecordCreated {
        id: record.id,
        source_kind: record.source_kind,
        created_by: record.created_by,
        action_item_id: None,
        conversation_id: None,
    });
    // Proposed, it waits for a person rather than for a slot, so the pool is not told.
    Ok(actions::publish(connection, events, written).await)
}

/// Withdraws `project_id`'s batch `batch_id` while no worker has started on it, saying `reason`,
/// as its Project Manager, and announces it.
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
    actions::cancel(
        connection,
        events,
        batch_id,
        Some(project_id),
        reason,
        PROJECT_MANAGER_USER_ID,
    )
    .await
    .map_err(|error| match error {
        ChangeError::NotFound => BatchError::Refused(format!(
            "batch {batch_id} is not a batch of this project. Call manager_batches for yours"
        )),
        ChangeError::Refused(reason) => BatchError::Refused(format!(
            "{reason}. Call manager_batches for where each stands"
        )),
        ChangeError::Invalid(reason) => BatchError::Refused(reason),
        ChangeError::Database(error) => BatchError::Database(error),
    })
}

/// The goal in `project_id`'s brief that `serves` names, as the brief spells it, or a refusal
/// that lists the goals there are and says to ask the person when the work serves none.
async fn goal_served(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    serves: &str,
) -> Result<String, BatchError> {
    let brief = project_brief::find(connection, project_id).await?;
    let goals = project_brief::goals(&brief.body);
    if let Some(goal) = project_brief::find_goal(&goals, serves) {
        return Ok(goal.to_owned());
    }
    Err(BatchError::Refused(refusal_for(
        serves.trim(),
        &goals,
        brief.id,
        brief.current_version,
    )))
}

/// Why a batch serving `serves` was refused, given the brief's `goals`: what the brief does name,
/// and that work outside it is the person's to decide. Never a way around the check: a goal is
/// added to the brief only when a person asked for it.
fn refusal_for(serves: &str, goals: &[String], brief_id: Uuid, version: i32) -> String {
    let brief = format!("this project's brief (page {brief_id}, version {version})");
    if goals.is_empty() {
        return format!(
            "{brief} names no goals yet, so no batch can serve one. Ask the person what the \
             project is for, with an action item, and once they say, record each goal they give \
             as a ### heading under ## Goals, quoting them, then submit against it."
        );
    }
    let named: Vec<String> = goals.iter().map(|goal| format!("\"{goal}\"")).collect();
    format!(
        "\"{serves}\" is not a goal in {brief}, whose goals are {}. Pass the one this work \
         serves as serves. If it serves none of them, it is outside the brief: ask the person \
         whether it belongs, with an action item, and submit it only once they add the goal or \
         ask you to.",
        named.join(", ")
    )
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
    use chrono::TimeDelta;

    use super::*;

    fn submission() -> Submission {
        Submission {
            title: "Annual pricing".to_owned(),
            goal: "Show annual pricing on the pricing page, as Ada asked.".to_owned(),
            serves: "Annual pricing".to_owned(),
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
    fn a_batch_names_the_goal_in_the_brief_it_serves() {
        let mut serving_nothing = submission();
        serving_nothing.serves = "  ".to_owned();
        assert!(check(&serving_nothing, Utc::now()).is_err_and(|reason| reason.contains("serves")));
        let mut too_long = submission();
        too_long.serves = "g".repeat(SERVES_MAX_CHARACTERS + 1);
        assert!(check(&too_long, Utc::now()).is_err_and(|reason| reason.contains("serves")));

        serde_json::from_value::<Submission>(json!({
            "title": "x", "goal": "y", "priority": "asap",
        }))
        .expect_err("serves is required");
    }

    #[test]
    fn a_refusal_names_the_briefs_goals_and_says_to_ask_the_person() {
        let brief_id = Uuid::now_v7();
        let goals = vec!["Annual pricing".to_owned(), "Spring launch".to_owned()];
        let refusal = refusal_for("Dark mode", &goals, brief_id, 4);
        assert!(refusal.contains("\"Dark mode\" is not a goal"), "{refusal}");
        assert!(
            refusal.contains(&format!("(page {brief_id}, version 4)")),
            "{refusal}"
        );
        assert!(
            refusal.contains("\"Annual pricing\", \"Spring launch\""),
            "{refusal}"
        );
        assert!(
            refusal.contains("ask the person whether it belongs"),
            "{refusal}"
        );

        let empty = refusal_for("Dark mode", &[], brief_id, 1);
        assert!(empty.contains("names no goals yet"), "{empty}");
        assert!(
            empty.contains("Ask the person what the project is for"),
            "{empty}"
        );
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
            "serves": "Reporting",
            "priority": "scheduled",
            "scheduledFor": "2026-11-05T09:00:00Z",
            "repositories": [{ "url": "https://github.com/org/repo", "baseBranch": "develop" }],
        }))
        .expect("reads");
        assert_eq!(read.priority, Priority::Scheduled);
        assert_eq!(read.repositories[0].base_branch.as_deref(), Some("develop"));
        assert!(read.record_ids.is_empty());

        serde_json::from_value::<Submission>(json!({
            "title": "x", "goal": "y", "serves": "z", "priority": "urgent",
        }))
        .expect_err("asap, normal, or scheduled");
        serde_json::from_value::<Submission>(json!({
            "title": "x", "goal": "y", "serves": "z", "priority": "asap", "worker": "me",
        }))
        .expect_err("nothing else");
    }
}
