// Copyright © 2026 Jalapeno Labs

//! `elysium_project_manager`: a Project Manager's own tools, for its one project.
//!
//! It reads its project's timeline, every owner's records on it, and its batches, and it submits
//! and withdraws batches for the worker pool. Every call is scoped to the turn's project and
//! checked against the database as it is now. What it submits names the turn as its provenance
//! and lands on the project's timeline, through `crate::project_managers::batches`, the one place
//! batches are stored. See `docs/engine.md`, Project Managers.

use chrono::{DateTime, Utc};
use futures_util::FutureExt as _;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    CallScope, Instructions, Tool, ToolContext, ToolError, ToolServer, internal, parse_arguments,
};
use crate::models::attribution::{ELYSIA_ACTOR, PROJECT_MANAGER_ACTOR};
use crate::models::timeline_record::{self, Attribution, TimelineRecord};
use crate::models::work_batch::{self, BatchState, WorkBatch};
use crate::project_managers::batches::{self, BatchError, Submission};

pub const SERVER: ToolServer = ToolServer {
    name: "elysium_project_manager",
    instructions: Instructions::Fixed(
        "Your project's timeline and its work batches. manager_timeline lists what is \
        on the project's timeline, newest first: meeting topics, messages, mail, your own turns \
        and batches. manager_record reads one record in full. manager_batches lists the batches \
        you submitted and where each stands; check it before submitting, so no work is \
        submitted twice. manager_submit_batch hands a piece of work to a worker: write its goal \
        so a worker who has seen nothing else can finish it, and name the records and action \
        items it came from. Batches wait in one queue for the whole workspace, which a person or \
        Elysia may reorder or pause. manager_cancel_batch withdraws a batch no worker has started \
        on yet.",
    ),
    tools: &TOOLS,
};

const TOOLS: [Tool; 5] = [
    Tool {
        name: "manager_timeline",
        description: "Lists the records on this project's timeline, newest first: each one's \
            id, kind, when, who it came from, title, summary, and the start of its text. Pass \
            before (the occurredAt of the last record listed) for older ones.",
        input_schema: timeline_schema,
        run: |context, scope, arguments| run_timeline(context, scope, arguments).boxed(),
    },
    Tool {
        name: "manager_record",
        description: "Reads one record on this project's timeline in full.",
        input_schema: record_schema,
        run: |context, scope, arguments| run_record(context, scope, arguments).boxed(),
    },
    Tool {
        name: "manager_batches",
        description: "Lists this project's work batches, newest first: each one's id, title, \
            priority, when it is scheduled, where it stands (queued for a slot, paused by a person \
            or Elysia, assigned a slot and waiting for its worker, running, succeeded, failed, or \
            cancelled), and what its worker reported or why it was cancelled. By default every \
            state; pass states for some.",
        input_schema: batches_schema,
        run: |context, scope, arguments| run_batches(context, scope, arguments).boxed(),
    },
    Tool {
        name: "manager_submit_batch",
        description: "Submits a work batch for a worker: a title, a goal complete enough to \
            work from alone, the repositories it needs, and a priority, asap, normal, or \
            scheduled with scheduledFor (an ISO 8601 time with its offset). recordIds and \
            actionItemIds name what called for it. It is put on the project's timeline. \
            Answers the stored batch.",
        input_schema: submit_schema,
        run: |context, scope, arguments| run_submit(context, scope, arguments).boxed(),
    },
    Tool {
        name: "manager_cancel_batch",
        description: "Withdraws one of this project's batches that no worker has started on \
            yet (queued, paused, or assigned), saying why.",
        input_schema: cancel_schema,
        run: |context, scope, arguments| run_cancel(context, scope, arguments).boxed(),
    },
];

/// How many records `manager_timeline` lists at most, and when the call does not say.
const TIMELINE_MAX: i64 = 50;
const TIMELINE_DEFAULT: i64 = 20;

/// How much of each record's text `manager_timeline` shows, in characters.
const TIMELINE_EXCERPT_CHARACTERS: usize = 600;

/// How much of a record's text `manager_record` shows, in characters: far more than a turn
/// needs from one record, and a bound on one answer.
const RECORD_MAX_CHARACTERS: usize = 40_000;

/// How many batches `manager_batches` lists.
const BATCHES_MAX: i64 = 100;

fn id_property(description: &str) -> Value {
    json!({ "type": "string", "format": "uuid", "description": description })
}

fn timeline_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "before": {
                "type": "string",
                "format": "date-time",
                "description": "Only records that happened before this time, for the next page.",
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": TIMELINE_MAX,
                "description": format!("The most records to list. Omit for {TIMELINE_DEFAULT}."),
            },
        },
        "additionalProperties": false,
    })
}

fn record_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "recordId": id_property("The record's id, from manager_timeline.") },
        "required": ["recordId"],
        "additionalProperties": false,
    })
}

fn batches_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "states": {
                "type": "array",
                "items": {
                    "type": "string",
                    "enum": [
                        "queued", "paused", "assigned", "running", "succeeded", "failed",
                        "cancelled",
                    ],
                },
                "minItems": 1,
                "uniqueItems": true,
                "description": "Only batches in these states. Omit for every state.",
            },
        },
        "additionalProperties": false,
    })
}

fn submit_schema() -> Value {
    let ids = |description: &str| {
        json!({
            "type": "array",
            "items": { "type": "string", "format": "uuid" },
            "maxItems": 50,
            "uniqueItems": true,
            "description": description,
        })
    };
    json!({
        "type": "object",
        "properties": {
            "title": {
                "type": "string",
                "minLength": 1,
                "maxLength": 200,
                "description": "A short name a person recognizes in a list.",
            },
            "goal": {
                "type": "string",
                "minLength": 1,
                "maxLength": 20000,
                "description": "What done looks like, for a worker who has seen nothing else: \
                    the context, what to change, how to check it, and who asked.",
            },
            "repositories": {
                "type": "array",
                "maxItems": 16,
                "items": {
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "minLength": 1,
                            "maxLength": 2048,
                            "description": "The Git URL to clone, such as \
                                https://github.com/org/repo.git.",
                        },
                        "baseBranch": {
                            "type": "string",
                            "minLength": 1,
                            "maxLength": 255,
                            "description": "The branch the work starts from. Omit for the \
                                remote's default.",
                        },
                    },
                    "required": ["url"],
                    "additionalProperties": false,
                },
                "description": "Every repository the work needs cloned.",
            },
            "priority": {
                "type": "string",
                "enum": ["asap", "normal", "scheduled"],
                "description": "asap when someone waits on it or it blocks other work, normal \
                    otherwise, scheduled when it must not start before scheduledFor.",
            },
            "scheduledFor": {
                "type": "string",
                "format": "date-time",
                "description": "When a scheduled batch is due, with its offset. Only for \
                    scheduled.",
            },
            "recordIds": ids("The timeline records that called for it."),
            "actionItemIds": ids("The action items it works on."),
        },
        "required": ["title", "goal", "priority"],
        "additionalProperties": false,
    })
}

fn cancel_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "batchId": id_property("The batch, from manager_batches."),
            "reason": {
                "type": "string",
                "minLength": 1,
                "maxLength": 2000,
                "description": "Why it is withdrawn.",
            },
        },
        "required": ["batchId", "reason"],
        "additionalProperties": false,
    })
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TimelineArguments {
    before: Option<DateTime<Utc>>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecordArguments {
    record_id: Uuid,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BatchesArguments {
    #[serde(default)]
    states: Vec<BatchState>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CancelArguments {
    batch_id: Uuid,
    reason: String,
}

async fn run_timeline(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let manager = scope.project_manager()?;
    let arguments: TimelineArguments = parse_arguments(arguments)?;
    let limit = arguments.limit.unwrap_or(TIMELINE_DEFAULT);
    if !(1..=TIMELINE_MAX).contains(&limit) {
        return Err(ToolError::Invalid(format!(
            "limit must be between 1 and {TIMELINE_MAX}"
        )));
    }
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let records = timeline_record::attributed_to(
        &mut connection,
        manager.project_id,
        arguments.before,
        limit,
    )
    .await
    .map_err(|error| internal(scope, "manager.timeline", &error))?;
    let listed: Vec<Value> = records
        .iter()
        .map(|(record, attribution)| record_view(record, attribution, TIMELINE_EXCERPT_CHARACTERS))
        .collect();
    let more = i64::try_from(listed.len()).is_ok_and(|count| count == limit);
    Ok(json!({ "records": listed, "more": more }))
}

async fn run_record(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let manager = scope.project_manager()?;
    let arguments: RecordArguments = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let found = timeline_record::attributed_among(
        &mut connection,
        manager.project_id,
        &[arguments.record_id],
    )
    .await
    .map_err(|error| internal(scope, "manager.record", &error))?;
    let Some((record, attribution)) = found.first() else {
        return Err(ToolError::NotFound(format!(
            "record {} is not on this project's timeline; call manager_timeline for the ones \
             that are",
            arguments.record_id
        )));
    };
    let mut view = record_view(record, attribution, RECORD_MAX_CHARACTERS);
    view["url"] = json!(record.url);
    Ok(json!({ "record": view }))
}

async fn run_batches(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let manager = scope.project_manager()?;
    let arguments: BatchesArguments = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let listed = work_batch::list_for_project(
        &mut connection,
        manager.project_id,
        &arguments.states,
        BATCHES_MAX,
    )
    .await
    .map_err(|error| internal(scope, "manager.batches", &error))?;
    let batches: Vec<Value> = listed.iter().map(batch_view).collect();
    Ok(json!({ "batches": batches }))
}

async fn run_submit(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let manager = scope.project_manager()?;
    let submission: Submission = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let batch = batches::submit(
        &mut connection,
        &context.events,
        manager.project_id,
        manager.turn_id,
        submission,
    )
    .await
    .map_err(|error| refused_or_internal(scope, "manager.submit_batch", error))?;
    Ok(json!({ "batch": batch_view(&batch) }))
}

async fn run_cancel(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let manager = scope.project_manager()?;
    let arguments: CancelArguments = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let batch = batches::cancel(
        &mut connection,
        &context.events,
        manager.project_id,
        arguments.batch_id,
        &arguments.reason,
    )
    .await
    .map_err(|error| refused_or_internal(scope, "manager.cancel_batch", error))?;
    Ok(json!({ "batch": batch_view(&batch) }))
}

fn refused_or_internal(scope: &CallScope, operation: &'static str, error: BatchError) -> ToolError {
    match error {
        BatchError::Refused(reason) => ToolError::Invalid(reason),
        BatchError::Database(error) => internal(scope, operation, &error),
    }
}

/// A record as the tools show it, its text cut to `characters`.
fn record_view(record: &TimelineRecord, attribution: &Attribution, characters: usize) -> Value {
    let text: String = record.body.chars().take(characters).collect();
    let cut = text.len() < record.body.len();
    let attributed_by = match attribution.attributed_by.as_str() {
        ELYSIA_ACTOR => "elysia",
        PROJECT_MANAGER_ACTOR => "you",
        _ => "a person",
    };
    json!({
        "id": record.id,
        "kind": record.source_kind,
        "occurredAt": record.occurred_at,
        "from": record.actor,
        "title": record.title,
        "summary": record.metadata["summary"].as_str(),
        "text": text,
        "textCut": cut,
        "attribution": {
            "confidence": attribution.confidence,
            "attributedBy": attributed_by,
        },
    })
}

/// A batch as the tools show it.
fn batch_view(batch: &WorkBatch) -> Value {
    json!({
        "id": batch.id,
        "title": batch.title,
        "goal": batch.goal,
        "repositories": batch.repositories,
        "priority": batch.priority,
        "scheduledFor": batch.scheduled_for,
        "state": batch.state,
        "result": batch.result,
        "recordIds": batch.source_record_ids,
        "actionItemIds": batch.action_item_ids,
        "submittedAt": batch.created_at,
        "startedAt": batch.started_at,
        "finishedAt": batch.finished_at,
    })
}
