// Copyright © 2026 Jalapeno Labs

//! Submitting and withdrawing a Project Manager's work batches: the rules a batch is held to,
//! and the one place it is stored, put on the timeline, and announced.
//!
//! Nothing a submission names is taken on trust: its repositories follow the rules a coding
//! session's do, a scheduled time is in the future and within a year, every record and message it
//! names as its provenance is on its project, and every action item it works on is led by its
//! project. An item can sit on several projects, but only its lead project's Project Manager works
//! it (`crate::models::action_item`, Projects and the lead). The rules are [`check`], a pure
//! function tested on its own; [`submit`] adds the checks that read the database. A person's edits
//! to a proposed batch are held to the same rules ([`check_edits`]).
//!
//! Its origin and generation are worked out from that provenance and the turn that submits it,
//! never asserted (`crate::models::work_batch::lineage`): a batch that names nothing, or that
//! would be past the generation limit, is refused with what to do instead.
//!
//! A submitted batch is proposed, waiting for a person to approve it before the pool may give it a
//! slot (`crate::models::work_batch::approval`), unless its project's level of autonomy lets work
//! that traces to a person queue at once (`crate::models::project_autonomy`). Found work is
//! proposed at every level.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::timeline;
use crate::database::schema::{action_items, project_manager_wakes};
use crate::models::project;
use crate::models::project_manager::WakeKind;
use crate::models::timeline_record::{self, TimelineRecord};
use crate::models::user::PROJECT_MANAGER_USER_ID;
use crate::models::work_batch::approval::Edits;
use crate::models::work_batch::lineage::{self, MAX_GENERATION, Provenance, Refusal};
use crate::models::work_batch::{
    self, BatchState, ChangeError, NewWorkBatch, Priority, Repository, WorkBatch, Written,
};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::routes::v1::coding_sessions::validate_repository_url;
use crate::worker_pool::actions;

/// The longest title, matching `work_batches.title`.
const TITLE_MAX_CHARACTERS: usize = 200;

/// The longest goal, matching `work_batches.goal`.
const TEXT_MAX_CHARACTERS: usize = 20_000;

/// The most repositories one batch needs, as a coding session clones at most.
const REPOSITORIES_MAX: usize = 16;

/// The most records, action items, or messages one batch names as its provenance. A batch is one
/// piece of work; more than this is several.
const PROVENANCE_MAX: usize = 50;

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
    /// The messages a person or Elysia sent its Project Manager that called for it.
    #[serde(default)]
    pub message_ids: Vec<Uuid>,
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
    check_repositories(&submission.repositories)?;
    work_batch::check_schedule(submission.priority, submission.scheduled_for, now)?;

    for (name, ids) in [
        ("recordIds", &submission.record_ids),
        ("actionItemIds", &submission.action_item_ids),
        ("messageIds", &submission.message_ids),
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

/// Submits `submission` for `project_id` from its Project Manager's turn `turn_id`: works out its
/// origin and generation, stores it, proposed or queued as its project's level of autonomy says,
/// puts it on the project's timeline, raises or updates the project's approval item when it is
/// proposed, and announces them.
///
/// # Errors
/// [`BatchError::Refused`] naming what to change, when [`check`] refuses it, it names a record or
/// message that is not on the project, it works on an action item the project does not lead, it
/// names nothing that called for it, or it would be past the generation limit; any database error.
pub async fn submit(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    project_id: Uuid,
    turn_id: Uuid,
    submission: Submission,
) -> Result<WorkBatch, BatchError> {
    check(&submission, Utc::now()).map_err(BatchError::Refused)?;
    refuse_unknown_provenance(connection, project_id, &submission).await?;
    let provenance = Provenance {
        records: &submission.record_ids,
        action_items: &submission.action_item_ids,
        messages: &submission.message_ids,
    };
    let roots = lineage::roots(connection, project_id, provenance).await?;
    let parents = lineage::parents(connection, project_id, Some(turn_id), provenance).await?;
    let lineage = lineage::decide(&roots, &parents).map_err(|refusal| {
        tracing::event!(
            name: "project_manager.batch.refused",
            tracing::Level::INFO,
            project.id = %project_id,
            project_manager.turn.id = %turn_id,
            refusal = %refusal,
            "a Project Manager's batch was refused for its lineage",
        );
        BatchError::Refused(refusal_words(&refusal))
    })?;

    let repositories = tidy_repositories(submission.repositories);
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
        source_message_ids: json!(submission.message_ids),
        lineage,
        created_by: PROJECT_MANAGER_USER_ID,
    };
    let stored = store(connection, new_batch, timeline::ACTOR).await?;
    Ok(announce(connection, events, stored).await)
}

/// A batch [`store`] wrote, for [`announce`] once its transaction commits.
#[derive(Debug)]
pub struct Stored {
    written: Written,
    record: TimelineRecord,
}

impl Stored {
    /// The batch as it was stored.
    pub const fn batch(&self) -> &WorkBatch {
        &self.written.batch
    }
}

/// Stores `new_batch`, proposed or queued as its project's level of autonomy says
/// (`work_batch::submit`), puts it on its project's timeline as submitted by `submitted_by`, and
/// raises or updates the project's approval item when it is proposed, in one transaction, or in
/// the caller's when it holds one. Its lineage is already decided: a Project Manager's from its
/// provenance ([`submit`]), or a person's accepting a finding (`crate::findings`), whose name
/// its record carries.
///
/// # Errors
/// Propagates any database error.
pub async fn store(
    connection: &mut AsyncPgConnection,
    new_batch: NewWorkBatch,
    submitted_by: &str,
) -> diesel::QueryResult<Stored> {
    connection
        .transaction(async |connection| {
            let mut written = work_batch::submit(connection, new_batch).await?;
            let record = timeline::record_batch(connection, &written.batch, submitted_by).await?;
            work_batch::set_timeline_record(connection, written.batch.id, record.id).await?;
            written.batch.timeline_record_id = Some(record.id);
            Ok(Stored { written, record })
        })
        .await
}

/// Announces a batch [`store`] wrote, once the transaction that wrote it committed: its record,
/// the batch, and its project's approval item, and tells the pool when the batch went straight to
/// the queue. Answers the batch.
pub async fn announce(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    stored: Stored,
) -> WorkBatch {
    let Stored { written, record } = stored;
    events.publish(&ServerEvent::TimelineRecordCreated {
        id: record.id,
        source_kind: record.source_kind,
        created_by: record.created_by,
        action_item_id: None,
        conversation_id: None,
    });
    let batch = actions::publish(connection, events, written).await;
    // A proposed batch waits for a person rather than for a slot, so only a queued one is the
    // pool's to hear about, which may hand it a slot at once, as an approval does.
    if batch.state == BatchState::Queued {
        events.signal(&Signal::WorkerPool);
    }
    batch
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

/// What a Project Manager is told when its batch's lineage is refused: why, and what to do
/// instead.
fn refusal_words(refusal: &Refusal) -> String {
    match refusal {
        Refusal::NoOrigin => "name what called for this batch: recordIds for the records on this \
            project's timeline, actionItemIds for the items it works on, or messageIds for the \
            messages sent to you. A batch that names nothing has no origin, and is refused"
            .to_owned(),
        Refusal::PastLimit {
            generation,
            parents,
        } => {
            let parents: Vec<String> = parents.iter().map(Uuid::to_string).collect();
            format!(
                "this batch would be generation {generation}: it follows batch {} in a chain of \
                 work each batch's worker turned up, and a chain stops at generation \
                 {MAX_GENERATION} so that work does not keep filing work. Do not submit it, and do \
                 not name other records to get around the limit. Say in your reply what you would \
                 do next and why, so the person decides whether it goes on",
                parents.join(", ")
            )
        }
    }
}

/// Refuses a submission naming a record not attributed to its project, an action item that is
/// deleted or that another project leads, or a message that was not sent to its Project Manager.
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
        let leads: HashMap<Uuid, Option<Uuid>> = action_items::table
            .filter(action_items::id.eq_any(&submission.action_item_ids))
            .filter(action_items::deleted_at.is_null())
            .select((action_items::id, action_items::lead_project_id))
            .load::<(Uuid, Option<Uuid>)>(connection)
            .await?
            .into_iter()
            .collect();
        for item_id in &submission.action_item_ids {
            let lead = leads.get(item_id).copied().flatten();
            if lead == Some(project_id) {
                continue;
            }
            let Some(lead) = lead else {
                return Err(BatchError::Refused(format!(
                    "action item {item_id} is not in this project or was deleted; call \
                     work_items for the ones that are"
                )));
            };
            let leader = project::find(connection, lead).await?;
            return Err(BatchError::Refused(lead_refusal(*item_id, &leader.name)));
        }
    }
    if !submission.message_ids.is_empty() {
        let found: HashSet<Uuid> = project_manager_wakes::table
            .filter(project_manager_wakes::project_id.eq(project_id))
            .filter(project_manager_wakes::kind.eq(WakeKind::Message))
            .filter(project_manager_wakes::id.eq_any(&submission.message_ids))
            .select(project_manager_wakes::id)
            .load::<Uuid>(connection)
            .await?
            .into_iter()
            .collect();
        if let Some(missing) = submission.message_ids.iter().find(|id| !found.contains(id)) {
            return Err(BatchError::Refused(format!(
                "message {missing} is not one sent to this project's Project Manager; each message \
                 in your prompt names its id"
            )));
        }
    }
    Ok(())
}

/// Why a batch for an item another project leads is refused: only the lead's Project Manager works
/// an item, so the work is left to it, or a person moves the lead.
fn lead_refusal(item_id: Uuid, lead_name: &str) -> String {
    format!(
        "action item {item_id} is led by the project {lead_name}, and only its Project Manager may \
         submit a batch for it; leave the item to that project, or ask a person to make this \
         project its lead on the item's page"
    )
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

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
            message_ids: Vec::new(),
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
    fn a_batch_for_an_item_led_elsewhere_says_whose_it_is_and_what_to_do() {
        let item = Uuid::now_v7();
        let refusal = lead_refusal(item, "Website");
        assert!(refusal.contains(&item.to_string()));
        assert!(refusal.contains("led by the project Website"));
        assert!(refusal.contains("only its Project Manager may submit a batch"));
        assert!(refusal.contains("make this project its lead"));
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

    #[test]
    fn a_project_manager_cannot_assert_an_origin_or_a_generation() {
        // Both are worked out from what a batch names; a submission saying either is refused
        // whole rather than read.
        for (field, value) in [("origin", json!("human_rooted")), ("generation", json!(0))] {
            let mut asserting = json!({
                "title": "x", "goal": "y", "priority": "asap",
                "recordIds": [Uuid::now_v7()],
            });
            asserting[field] = value;
            serde_json::from_value::<Submission>(asserting).expect_err(field);
        }
    }

    #[test]
    fn messages_are_named_once_each() {
        let mut twice = submission();
        let id = Uuid::now_v7();
        twice.message_ids = vec![id, id];
        assert!(check(&twice, Utc::now()).is_err_and(|reason| reason.contains("messageIds")));
    }
}
