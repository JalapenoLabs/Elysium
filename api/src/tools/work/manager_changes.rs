// Copyright © 2026 Jalapeno Labs

//! `manager_make_changes`: a Project Manager changes its project's action items and files their
//! issues in the project's tracker.
//!
//! Every change is a changeset operation, as every agent's is, so the person reviews, applies,
//! and undoes a Project Manager's work the way they do anyone's, and each change it applies lands
//! on the project's timeline with the record that caused it. What its project allows decides
//! the rest (`projects.manager_writes_directly`):
//!
//! - **Proposing**, the default: the changeset waits for a person's approval, and nothing is
//!   written in Elysium or any tracker until then.
//! - **Writing directly**: the changeset is approved and applied at once, by the Project
//!   Managers' machine, so an item it creates starts in the inbox for the person to accept.
//!   Comments, status changes, and new issues reach the trackers through the watcher's outbox,
//!   like the person's own.
//!
//! What it can change, in Elysium's terms: create, update, resolve, dismiss, and comment on
//! items, move them between initiatives, create initiatives, and file an item's own issue
//! (`file-issue`). An issue goes to the project's tracker, which the project names or its
//! precedent decides (`crate::project_managers::trackers`); the Project Manager never chooses
//! one. Resolving an item moves its linked issues to done, as the person resolving it would.
//!
//! Each change names the record it came from (`causeRecordId`), which must be on the project's
//! timeline. A retried turn proposes what it proposed before, so an item an earlier changeset
//! already creates from the same record under the same title is refused, naming it, rather than
//! made twice; the filing outbox keeps a retried filing from opening a second issue.

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
use crate::models::changeset::{self, ChangesetDecision, Staged, Written};
use crate::models::user::PROJECT_MANAGER_USER_ID;
use crate::models::{action_item_issue_filing, action_item_link, project, timeline_record};
use crate::project_managers::trackers;
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
    cause_record_id: Uuid,
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

/// A change read, with why it is made and the record it came from.
#[derive(Debug)]
struct Drafted {
    draft: Draft,
    reason: String,
    quote: Option<String>,
    source: Option<String>,
    cause_record_id: Uuid,
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
        .manager_writes_directly
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

/// The operations to stage: each change scoped to the project, its cause checked to be on the
/// project's timeline, its item's issue filed in the project's tracker, and a create made before
/// from the same record refused.
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

    let causes: Vec<Uuid> = drafts
        .iter()
        .map(|drafted| drafted.cause_record_id)
        .collect::<BTreeSet<Uuid>>()
        .into_iter()
        .collect();
    let on_timeline: BTreeSet<Uuid> =
        timeline_record::attributed_among(connection, scope.project_id, &causes)
            .await?
            .into_iter()
            .map(|(record, _attribution)| record.id)
            .collect();

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
        let cause = drafted.cause_record_id;
        if !on_timeline.contains(&cause) {
            return Err(refused(
                position,
                format!(
                    "record {cause} is not on this project's timeline; name the segment or \
                     message it came from, from manager_timeline"
                ),
            ));
        }
        let operation = match drafted.draft {
            Draft::Operation(operation) => {
                if let Operation::CreateItem(create) = &operation
                    && let Some(earlier) =
                        changeset::earlier_create(connection, proposer, cause, &create.title)
                            .await?
                {
                    let item = earlier
                        .item_id
                        .map_or_else(String::new, |item_id| format!(" as item {item_id}"));
                    return Err(refused(
                        position,
                        format!(
                            "\"{}\" was already made from this record, in changeset {}{item}; \
                             act on that item instead of making it again",
                            create.title.trim(),
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
            cause_record_id: Some(cause),
        });
    }
    Ok(operations)
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
