// Copyright © 2026 Jalapeno Labs

//! `manager_make_changes`: a Project Manager changes its project's action items and files their
//! issues in the project's tracker.
//!
//! Every change is a changeset operation, as every agent's is, so the person reviews, applies,
//! and undoes a Project Manager's work the way they do anyone's, and each change it applies lands
//! on the project's timeline with the record that caused it. Its project's level of autonomy
//! decides the rest (`crate::models::project_autonomy`):
//!
//! - **Proposing**, at Approve every batch, the default, and Approve found work only: the
//!   changeset waits for a person's approval, and nothing is written in Elysium or any tracker
//!   until then.
//! - **Writing directly**, at Full autonomy: the changeset is approved and applied at once, by
//!   the Project Managers' machine, so an item it creates starts in the inbox for the person to
//!   accept. Comments, status changes, and new issues reach the trackers through the watcher's
//!   outbox, like the person's own.
//!
//! What it can change, in Elysium's terms: create, update, resolve, dismiss, and comment on
//! items, move them between initiatives, create initiatives, and file an item's own issue
//! (`file-issue`). An issue goes to the project's tracker, which the project names or its
//! precedent decides (`crate::project_managers::trackers`); the Project Manager never chooses
//! one. Resolving an item moves its linked issues to done, as the person resolving it would.
//!
//! Each change names what it came from: the record (`causeRecordId`), which must be on the
//! project's timeline, a task heard in a meeting for the project (`taskId`), or both. A create
//! naming a task is that task's action item once it applies (`models::meeting_task`), which the
//! meeting's page counts and links back to the words it was heard in. A retried turn proposes
//! what it proposed before, so an item an earlier changeset already creates from the same task,
//! or failing one the same record, under the same title is refused, naming it, rather than made
//! twice; the filing outbox keeps a retried filing from opening a second issue.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::propose::{self, Proposing};
use super::{Failure, WorkAgent, WorkScope, find_item};
use crate::action_items::changesets::{
    FileIssue, LinkReads, OPERATIONS_MAX, Operation, ProposedOperation, Proposer, Target,
    TrackerTarget,
};
use crate::models::changeset::{self, ChangesetDecision, CreatedFrom, Staged, Written};
use crate::models::user::PROJECT_MANAGER_USER_ID;
use crate::models::{
    action_item_issue_filing, action_item_link, meeting_task, project, timeline_record,
};
use crate::project_managers::{tasks, trackers};
use crate::routes::v1::changesets::{publish_changeset, publish_touched};
use crate::tools::{CallScope, ToolContext, ToolError, internal, parse_arguments};

/// The operation name logs use for this tool.
const OPERATION: &str = "manager.make_changes";

/// What the tool answers once its changes are staged for review.
const STAGED_MESSAGE: &str = "Staged for a person's review. Nothing changes, in Elysium or any \
    tracker, until they approve it, in whole or in part; you are not told when they do. Say in \
    your reply what you proposed.";

/// What the tool answers once its changes applied.
const APPLIED_MESSAGE: &str = "Applied. Each change is on the project's timeline with the record \
    it came from. Comments, status changes, and new issues reach the trackers within a minute; an \
    operation that failed says why and wrote nothing. Say in your reply what you changed.";

/// The schema, a changeset's operations with the record each came from.
pub(in crate::tools) fn schema() -> Value {
    propose::schema_for(Proposing::ProjectManager)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Arguments {
    summary: String,
    operations: Vec<ProposedChange>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProposedChange {
    /// An [`Operation`], or a `file-issue` without its tracker; read by [`parse_change`].
    change: Value,
    reason: String,
    quote: Option<String>,
    source: Option<String>,
    /// What it came from: a record, a meeting's task, or both; [`prepare`] refuses neither.
    cause_record_id: Option<Uuid>,
    task_id: Option<Uuid>,
}

/// A `file-issue` as a Project Manager sends it: the item alone, since the project's tracker is
/// the project's to decide.
#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum FileIssueChange {
    FileIssue { item: Target },
}

/// A change as the Project Manager sent it, before its tracker is resolved.
#[derive(Debug)]
enum Draft {
    Operation(Operation),
    FileIssue { item: Target },
}

/// A change read, with why it is made and what it came from: the record, the meeting's task, or
/// both.
#[derive(Debug)]
struct Drafted {
    draft: Draft,
    reason: String,
    quote: Option<String>,
    source: Option<String>,
    cause_record_id: Option<Uuid>,
    task_id: Option<Uuid>,
}

/// Reads one change, refusing what a Project Manager may not make.
fn parse_change(change: Value) -> Result<Draft, String> {
    if change["kind"] == json!("file-issue") {
        let FileIssueChange::FileIssue { item } =
            serde_json::from_value(change).map_err(|error| error.to_string())?;
        return Ok(Draft::FileIssue { item });
    }
    let mut operation: Operation =
        serde_json::from_value(change).map_err(|error| error.to_string())?;
    if matches!(operation, Operation::Link(_)) {
        return Err(
            "an item's issue is filed with file-issue; other links are a person's to add"
                .to_owned(),
        );
    }
    if operation
        .project_ids_mut()
        .is_some_and(|project_ids| !project_ids.is_empty())
    {
        return Err("leave projectIds out: what you create joins this project".to_owned());
    }
    Ok(Draft::Operation(operation))
}

/// What a Project Manager's changes came to.
#[derive(Debug)]
enum Made {
    /// Staged for a person's review.
    Staged(Staged),
    /// Applied at once, as the project lets it.
    Applied(Written),
}

/// Parses the Project Manager's changes, makes them, then tells every client and wakes the
/// watcher for what they owe the trackers.
pub(in crate::tools) async fn run(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let manager = scope.project_manager()?;
    let arguments: Arguments = parse_arguments(arguments)?;
    if arguments.operations.len() > OPERATIONS_MAX {
        return Err(ToolError::Invalid(format!(
            "make at most {OPERATIONS_MAX} changes at once; split the rest into another call"
        )));
    }
    let mut drafts = Vec::with_capacity(arguments.operations.len());
    for (proposed, position) in arguments.operations.into_iter().zip(1..) {
        let draft = parse_change(proposed.change)
            .map_err(|refusal| ToolError::Invalid(format!("operation {position}: {refusal}")))?;
        drafts.push(Drafted {
            draft,
            reason: proposed.reason,
            quote: proposed.quote,
            source: proposed.source,
            cause_record_id: proposed.cause_record_id,
            task_id: proposed.task_id,
        });
    }

    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let now = Utc::now();
    let made = make_changes(
        &mut connection,
        manager.project_id,
        arguments.summary,
        drafts,
        now,
    )
    .await
    .map_err(|failure| failure.into_tool_error(scope, OPERATION))?;
    match made {
        Made::Staged(staged) => {
            let mut answer = propose::staged_answer(&staged);
            answer["message"] = json!(STAGED_MESSAGE);
            publish_changeset(&context.events, staged);
            Ok(answer)
        }
        Made::Applied(Written { staged, touched }) => {
            publish_touched(&context.events, &mut connection, touched, now)
                .await
                .map_err(|error| internal(scope, OPERATION, &error))?;
            context.links.wake_watcher();
            let answer = applied_answer(&staged);
            publish_changeset(&context.events, staged);
            Ok(answer)
        }
    }
}

/// Checks and stages the Project Manager's changes as one changeset, and applies it at once
/// when its project lets it write. Publishes nothing; the caller tells clients.
async fn make_changes(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    summary: String,
    drafts: Vec<Drafted>,
    now: DateTime<Utc>,
) -> Result<Made, Failure> {
    let scope = WorkScope {
        agent: WorkAgent::ProjectManager,
        project_id,
        action_item_id: None,
    };
    let operations = prepare(connection, scope, drafts).await?;
    let staged = propose::stage(connection, scope, summary, operations, now).await?;
    if !project::find(connection, project_id)
        .await?
        .autonomy
        .writes_directly()
    {
        return Ok(Made::Staged(staged));
    }

    let id = staged.changeset.id;
    changeset::decide(connection, id, ChangesetDecision::Approved, None).await?;
    // A Project Manager proposes no links, so there is no target to read before applying.
    let written = changeset::apply(
        connection,
        id,
        LinkReads::new(),
        PROJECT_MANAGER_USER_ID,
        now,
    )
    .await?;
    Ok(Made::Applied(written))
}

/// The operations to stage: each change scoped to the project, what it came from checked (a
/// record on the project's timeline, a task heard in a meeting for the project, or both), its
/// item's issue filed in the project's tracker, and a create made before from the same task or
/// record refused.
async fn prepare(
    connection: &mut AsyncPgConnection,
    scope: WorkScope,
    drafts: Vec<Drafted>,
) -> Result<Vec<ProposedOperation>, Failure> {
    let refused = |position: usize, refusal: String| {
        Failure::Refused(ToolError::Invalid(format!(
            "operation {position}: {refusal}"
        )))
    };

    let causes = Causes::read(connection, scope.project_id, &drafts).await?;

    let files_issues = drafts
        .iter()
        .any(|drafted| matches!(drafted.draft, Draft::FileIssue { .. }));
    let tracker = if files_issues {
        Some(trackers::resolve(connection, scope.project_id).await?)
    } else {
        None
    };

    let proposer = Proposer::ProjectManager(scope.project_id);
    let mut operations = Vec::with_capacity(drafts.len());
    for (drafted, position) in drafts.into_iter().zip(1_usize..) {
        let from = causes
            .check(&drafted)
            .map_err(|refusal| refused(position, refusal))?;
        let operation = match drafted.draft {
            Draft::Operation(operation) => {
                if let Operation::CreateItem(create) = &operation
                    && let Some(earlier) =
                        changeset::earlier_create(connection, proposer, from, &create.title).await?
                {
                    let item = earlier
                        .item_id
                        .map_or_else(String::new, |item_id| format!(" as item {item_id}"));
                    return Err(refused(
                        position,
                        format!(
                            "\"{}\" was already made from this {}, in changeset {}{item}; act on \
                             that item instead of making it again",
                            create.title.trim(),
                            match from {
                                CreatedFrom::Task(_) => "task",
                                CreatedFrom::Record(_) => "record",
                            },
                            earlier.changeset_id
                        ),
                    ));
                }
                operation
            }
            Draft::FileIssue { item } => {
                let tracker = match tracker.as_ref().expect("resolved when any change files") {
                    Ok(tracker) => tracker,
                    Err(reason) => {
                        return Err(refused(position, format!("cannot file an issue: {reason}")));
                    }
                };
                if let Target::Existing(existing) = item {
                    already_filed(connection, scope, existing.id)
                        .await?
                        .map_or(Ok(()), |refusal| Err(refused(position, refusal)))?;
                }
                Operation::FileIssue(FileIssue {
                    item,
                    tracker: TrackerTarget {
                        provider: tracker.place.provider,
                        credential_id: tracker.credential_id,
                        destination: tracker.place.destination.clone(),
                    },
                })
            }
        };
        operations.push(ProposedOperation {
            operation,
            reason: drafted.reason,
            quote: drafted.quote,
            source: drafted.source,
            cause_record_id: drafted.cause_record_id,
            meeting_task_id: drafted.task_id,
        });
    }
    Ok(operations)
}

/// What the changes of one call came from, as the database says: the records among them on the
/// project's timeline, and the tasks among them heard in meetings for the project.
struct Causes {
    on_timeline: BTreeSet<Uuid>,
    projects_tasks: BTreeSet<Uuid>,
}

impl Causes {
    async fn read(
        connection: &mut AsyncPgConnection,
        project_id: Uuid,
        drafts: &[Drafted],
    ) -> diesel::QueryResult<Self> {
        let records: Vec<Uuid> = drafts
            .iter()
            .filter_map(|drafted| drafted.cause_record_id)
            .collect::<BTreeSet<Uuid>>()
            .into_iter()
            .collect();
        let named_tasks: Vec<Uuid> = drafts
            .iter()
            .filter_map(|drafted| drafted.task_id)
            .collect::<BTreeSet<Uuid>>()
            .into_iter()
            .collect();
        let on_timeline = timeline_record::attributed_among(connection, project_id, &records)
            .await?
            .into_iter()
            .map(|(record, _attribution)| record.id)
            .collect();
        let projects_tasks =
            meeting_task::standing_for_project(connection, project_id, &named_tasks)
                .await?
                .into_iter()
                .map(|task| task.mark_id)
                .collect();
        Ok(Self {
            on_timeline,
            projects_tasks,
        })
    }

    /// What `drafted` came from, which an earlier create is looked for by: its task when it names
    /// one, the more particular cause, so an item is made once per task, else its record. Refuses
    /// a change that names neither, a record not on the project's timeline, or a task that is not
    /// the project's.
    fn check(&self, drafted: &Drafted) -> Result<CreatedFrom, String> {
        if let Some(cause) = drafted.cause_record_id
            && !self.on_timeline.contains(&cause)
        {
            return Err(format!(
                "record {cause} is not on this project's timeline; name the excerpt or message it \
                 came from, from manager_timeline"
            ));
        }
        if let Some(task_id) = drafted.task_id
            && !self.projects_tasks.contains(&task_id)
        {
            return Err(tasks::TaskError::NotThisProjects(task_id).to_string());
        }
        match (drafted.task_id, drafted.cause_record_id) {
            (Some(task_id), _) => Ok(CreatedFrom::Task(task_id)),
            (None, Some(record_id)) => Ok(CreatedFrom::Record(record_id)),
            (None, None) => Err("name what it came from: causeRecordId, a record on this \
                 project's timeline from manager_timeline, or taskId, a task your prompt lists \
                 under Tasks heard in meetings"
                .to_owned()),
        }
    }
}

/// Why an existing item of the project cannot get an issue filed, if it cannot: it already
/// follows one, or one is already being filed for it.
async fn already_filed(
    connection: &mut AsyncPgConnection,
    scope: WorkScope,
    item_id: Uuid,
) -> Result<Option<String>, Failure> {
    find_item(connection, scope, item_id).await?;
    let links = action_item_link::list_for_item(connection, item_id).await?;
    if let Some(primary) = links.iter().find(|link| link.is_primary) {
        return Ok(Some(format!(
            "item {item_id} already follows {}; comment on it or change its status instead",
            primary.external_key
        )));
    }
    if action_item_issue_filing::for_item(connection, item_id)
        .await?
        .is_some()
    {
        return Ok(Some(format!(
            "an issue is already being filed for item {item_id}"
        )));
    }
    Ok(None)
}

/// What the Project Manager is told once its changes applied: how each operation ended.
fn applied_answer(staged: &Staged) -> Value {
    let operations: Vec<Value> = staged
        .operations
        .iter()
        .map(|row| {
            json!({
                "position": row.position,
                "kind": row.operation["kind"],
                "outcome": row.outcome,
                "error": row.error,
                "result": row.result,
            })
        })
        .collect();
    json!({
        "changeset": {
            "id": staged.changeset.id,
            "state": staged.changeset.state,
            "summary": staged.changeset.summary,
            "operations": operations,
        },
        "message": APPLIED_MESSAGE,
    })
}

#[cfg(test)]
mod tests;
