// Copyright © 2026 Jalapeno Labs

//! `work_propose_changes`: the agent proposes changes to its project's items and
//! initiatives as a changeset, which waits for the user's review.
//!
//! Nothing the agent proposes is written until the user approves it. A proposal is checked
//! whole before anything is stored: every item and initiative it names must be live and in
//! the session's project, as every other work tool requires, and an operation that names
//! another must name an earlier one that creates the right record. Items and initiatives the
//! agent proposes join the session's project.
//!
//! The agent speaks Elysium's terms, never a provider's, so it links a pull request by its
//! URL (`link-pull-request`), read through the session's GitHub token when proposed, and
//! stored as the changeset's `link` operation.

use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{Failure, WorkScope, find_initiative, find_item, pull_request_credential};
use crate::action_items::changesets::{
    AddLink, OPERATIONS_MAX, Operation, Proposal, ProposedOperation, Proposer, Subject, Target,
};
use crate::action_items::links::LinkError;
use crate::github::issues::IssueRef;
use crate::models::action_item_link::{LinkKind, LinkProvider};
use crate::models::changeset::{self, Staged};
use crate::routes::v1::action_items::LinkTarget;
use crate::routes::v1::changesets::publish_changeset;
use crate::tools::{CallScope, ToolContext, ToolError, internal, parse_arguments};

/// What the tool answers after staging, so the agent does not wait for the outcome.
const STAGED_MESSAGE: &str = "Staged for the user's review. Nothing changes until the user \
    approves it, in whole or in part; you are not told when they do. Tell the user in your \
    reply that you proposed these changes.";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ProposeArguments {
    summary: String,
    operations: Vec<ProposedChange>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProposedChange {
    /// An [`Operation`], or a `link-pull-request`; read by [`parse_change`].
    change: Value,
    reason: String,
    quote: Option<String>,
    source: Option<String>,
}

/// The one change an agent sends that is not an [`Operation`] as stored: a pull request
/// named by its URL, which becomes a `link` through the session's GitHub token.
#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum PullRequestChange {
    LinkPullRequest { item: Target, url: String },
}

/// A change as the agent sent it, before the pull requests it names are read.
#[derive(Debug)]
enum Draft {
    Operation(Operation),
    LinkPullRequest { item: Target, reference: IssueRef },
}

/// A change as the agent sent it, with why it is proposed.
#[derive(Debug)]
struct Drafted {
    draft: Draft,
    reason: String,
    quote: Option<String>,
    source: Option<String>,
}

/// Reads one change, refusing what an agent may not propose.
fn parse_change(change: Value) -> Result<Draft, String> {
    if change["kind"] == json!("link-pull-request") {
        let PullRequestChange::LinkPullRequest { item, url } =
            serde_json::from_value(change).map_err(|error| error.to_string())?;
        let reference = IssueRef::from_pull_request_url(&url).ok_or_else(|| {
            "url is not a GitHub pull request; one reads \
             https://github.com/owner/name/pull/12"
                .to_owned()
        })?;
        return Ok(Draft::LinkPullRequest { item, reference });
    }

    let mut operation: Operation =
        serde_json::from_value(change).map_err(|error| error.to_string())?;
    if matches!(operation, Operation::Link(_)) {
        return Err(
            "link a pull request with link-pull-request and its URL; other links are the user's \
             to add"
                .to_owned(),
        );
    }
    if operation
        .project_ids_mut()
        .is_some_and(|project_ids| !project_ids.is_empty())
    {
        return Err(
            "leave projectIds out: what you propose joins this session's project".to_owned(),
        );
    }
    Ok(Draft::Operation(operation))
}

/// Parses, checks, and stages the agent's proposal, then tells every client about it.
pub(super) async fn run(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    const OPERATION: &str = "work.propose_changes";

    let arguments: ProposeArguments = parse_arguments(arguments)?;
    if arguments.operations.len() > OPERATIONS_MAX {
        return Err(ToolError::Invalid(format!(
            "propose at most {OPERATIONS_MAX} operations at once; split the rest into another \
             proposal"
        )));
    }
    let mut drafts = Vec::with_capacity(arguments.operations.len());
    for (proposed, position) in arguments.operations.into_iter().zip(1..) {
        let ProposedChange {
            change,
            reason,
            quote,
            source,
        } = proposed;
        let draft = parse_change(change)
            .map_err(|refusal| ToolError::Invalid(format!("operation {position}: {refusal}")))?;
        drafts.push(Drafted {
            draft,
            reason,
            quote,
            source,
        });
    }

    let work_scope = WorkScope::from(scope);
    let has_pull_requests = drafts
        .iter()
        .any(|drafted| matches!(drafted.draft, Draft::LinkPullRequest { .. }));
    let credential_id = if has_pull_requests {
        let mut connection = context
            .database
            .get()
            .await
            .map_err(|error| internal(scope, "database.connect", &error))?;
        Some(
            pull_request_credential(&mut connection, work_scope)
                .await
                .map_err(|failure| failure.into_tool_error(scope, OPERATION))?,
        )
    } else {
        None
    };

    let mut operations = Vec::with_capacity(drafts.len());
    for (drafted, position) in drafts.into_iter().zip(1..) {
        let operation = match drafted.draft {
            Draft::Operation(operation) => operation,
            Draft::LinkPullRequest { item, reference } => {
                let credential_id =
                    credential_id.expect("read above whenever a pull request is named");
                read_pull_request(context, scope, credential_id, item, &reference)
                    .await
                    .map_err(|error| match error {
                        ToolError::Internal => ToolError::Internal,
                        refused => ToolError::Invalid(format!("operation {position}: {refused}")),
                    })?
            }
        };
        operations.push(ProposedOperation {
            operation,
            reason: drafted.reason,
            quote: drafted.quote,
            source: drafted.source,
        });
    }

    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let staged = stage(
        &mut connection,
        work_scope,
        arguments.summary,
        operations,
        Utc::now(),
    )
    .await
    .map_err(|failure| failure.into_tool_error(scope, OPERATION))?;
    let answer = staged_answer(&staged);
    publish_changeset(&context.events, staged);
    Ok(answer)
}

/// Reads the pull request through the session's token, so a proposal never names one the
/// token cannot see, and answers the link operation for it.
async fn read_pull_request(
    context: &ToolContext,
    scope: &CallScope,
    credential_id: Uuid,
    item: Target,
    reference: &IssueRef,
) -> Result<Operation, ToolError> {
    let target = LinkTarget {
        provider: LinkProvider::Github,
        credential_id,
        kind: LinkKind::PullRequest,
        reference: reference.to_string(),
    };
    context
        .links
        .provider(LinkProvider::Github)
        .find(credential_id, LinkKind::PullRequest, &target.reference)
        .await
        .map_err(|error| match error {
            LinkError::Invalid(message)
            | LinkError::Forbidden(message)
            | LinkError::NotFound(message) => ToolError::Invalid(message),
            LinkError::Upstream(message) => ToolError::Provider(message),
            LinkError::Internal(error) => internal(scope, "work.propose_changes", &error),
        })?;
    Ok(Operation::Link(AddLink { item, target }))
}

/// Checks that everything the operations name is in the session's project, puts what they
/// create in it, and stores the changeset as the session's.
pub(super) async fn stage(
    connection: &mut AsyncPgConnection,
    scope: WorkScope,
    summary: String,
    operations: Vec<ProposedOperation>,
    now: DateTime<Utc>,
) -> Result<Staged, Failure> {
    let mut scoped = Vec::with_capacity(operations.len());
    for mut proposed in operations {
        for (target, subject) in proposed.operation.targets() {
            let Target::Existing(existing) = target else {
                continue;
            };
            match subject {
                Subject::Item => {
                    find_item(connection, scope, existing.id).await?;
                }
                Subject::Initiative => {
                    find_initiative(connection, scope, existing.id).await?;
                }
            }
        }
        if let Some(project_ids) = proposed.operation.project_ids_mut() {
            *project_ids = vec![scope.project_id];
        }
        scoped.push(proposed);
    }

    let proposal = Proposal {
        proposer: Proposer::Session(scope.session_id),
        project_id: Some(scope.project_id),
        summary,
        operations: scoped,
    };
    Ok(changeset::propose(connection, proposal, now).await?)
}

/// What the agent is told was staged: each operation's position, kind, and the earlier
/// operations it depends on.
fn staged_answer(staged: &Staged) -> Value {
    let parsed: Vec<Operation> = staged
        .operations
        .iter()
        .map(changeset::ChangesetOperation::parsed)
        .collect();
    let dependencies = crate::action_items::changesets::dependencies(&parsed);
    let operations: Vec<Value> = staged
        .operations
        .iter()
        .zip(dependencies)
        .map(|(row, depends_on)| {
            json!({
                "position": row.position,
                "kind": row.operation["kind"],
                "dependsOn": depends_on,
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
        "message": STAGED_MESSAGE,
    })
}

/// The tool's arguments as a JSON Schema: a summary and the operations, each a change with
/// its reason and, where there is one, the quote and source it came from.
pub(super) fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "summary": text_schema(500, "One line saying what the proposal is for."),
            "operations": {
                "type": "array",
                "minItems": 1,
                "maxItems": OPERATIONS_MAX,
                "description": "The changes, applied in this order once approved.",
                "items": {
                    "type": "object",
                    "properties": {
                        "change": { "oneOf": change_schemas() },
                        "reason": text_schema(2000, "Why the user should make this change."),
                        "quote": text_schema(
                            2000,
                            "The words it came from, if any, quoted exactly.",
                        ),
                        "source": text_schema(
                            500,
                            "Where those words are, such as a file and line.",
                        ),
                    },
                    "required": ["change", "reason"],
                    "additionalProperties": false,
                },
            },
        },
        "required": ["summary", "operations"],
        "additionalProperties": false,
    })
}

/// One schema per kind of change an agent may propose.
fn change_schemas() -> Vec<Value> {
    let priority = json!({ "type": "string", "enum": ["urgent", "high", "normal", "low"] });
    let notes = json!({ "type": "string", "maxLength": 20000 });
    vec![
        change_schema(
            "create-item",
            json!({
                "title": text_schema(500, "The item's title."),
                "notes": notes,
                "priority": priority,
                "dueAt": date_schema("When it is due, with an offset."),
                "initiatives": {
                    "type": "array",
                    "items": target_schema("initiative"),
                    "description": "Initiatives it joins.",
                },
            }),
            &["title"],
        ),
        change_schema(
            "update-item",
            json!({
                "item": target_schema("item"),
                "title": text_schema(500, "A new title."),
                "notes": notes,
                "priority": priority,
                "dueAt": date_schema("A new due date, or null to remove it."),
            }),
            &["item"],
        ),
        change_schema(
            "resolve-item",
            json!({ "item": target_schema("item") }),
            &["item"],
        ),
        change_schema(
            "dismiss-item",
            json!({ "item": target_schema("item") }),
            &["item"],
        ),
        change_schema(
            "comment",
            json!({
                "item": target_schema("item"),
                "body": text_schema(20000, "The comment, in plain text or Markdown."),
            }),
            &["item", "body"],
        ),
        change_schema(
            "link-pull-request",
            json!({
                "item": target_schema("item"),
                "url": text_schema(
                    2048,
                    "The pull request's URL, such as https://github.com/owner/name/pull/12.",
                ),
            }),
            &["item", "url"],
        ),
        change_schema(
            "add-to-initiative",
            json!({ "item": target_schema("item"), "initiative": target_schema("initiative") }),
            &["item", "initiative"],
        ),
        change_schema(
            "remove-from-initiative",
            json!({ "item": target_schema("item"), "initiative": target_schema("initiative") }),
            &["item", "initiative"],
        ),
        change_schema(
            "create-initiative",
            json!({
                "name": text_schema(200, "The initiative's name."),
                "description": notes,
                "targetAt": date_schema("When it should be achieved, with an offset."),
            }),
            &["name"],
        ),
    ]
}

/// A change of one `kind` with its `properties`, the `required` ones and its kind required.
fn change_schema(kind: &str, mut properties: Value, required: &[&str]) -> Value {
    properties["kind"] = json!({ "const": kind });
    let mut required = required.to_vec();
    required.push("kind");
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

/// An item or initiative a change names: one that exists, or one an earlier change creates.
fn target_schema(what: &str) -> Value {
    json!({
        "description": format!(
            "The {what}: {{\"id\": ...}} for one that exists, or {{\"operation\": n}} for the \
             {what} operation n of this proposal creates, counting from 1."
        ),
        "oneOf": [
            {
                "type": "object",
                "properties": { "id": { "type": "string", "format": "uuid" } },
                "required": ["id"],
                "additionalProperties": false,
            },
            {
                "type": "object",
                "properties": { "operation": { "type": "integer", "minimum": 1 } },
                "required": ["operation"],
                "additionalProperties": false,
            },
        ],
    })
}

fn text_schema(max: usize, description: &str) -> Value {
    json!({ "type": "string", "minLength": 1, "maxLength": max, "description": description })
}

fn date_schema(description: &str) -> Value {
    json!({ "type": ["string", "null"], "format": "date-time", "description": description })
}

#[cfg(test)]
mod tests;
