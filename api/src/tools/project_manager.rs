// Copyright © 2026 Jalapeno Labs

//! `elysium_project_manager`: a Project Manager's own tools, for its one project.
//!
//! It reads its project's timeline, every owner's records on it, and its batches, and it submits
//! and withdraws batches for the worker pool. It also keeps its project's promises: it lists them,
//! ties each to the work that keeps it, and drafts a follow-up once that work is done, through
//! `crate::project_managers::promises`. And it changes its project's action items and files their
//! issues in the project's tracker (`manager_make_changes`, in
//! `crate::tools::work::manager_changes`, beside the other work tools whose staging it shares).
//! Every call is scoped to the turn's project and checked against the database as it is now. What
//! it submits names the turn as its provenance and lands on the project's timeline, through
//! `crate::project_managers::batches`, the one place batches are stored. See `docs/engine.md`,
//! Project Managers and Follow-ups.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use futures_util::FutureExt as _;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    CallScope, Instructions, Tool, ToolContext, ToolError, ToolServer, internal, parse_arguments,
};
use crate::models::attribution::{ELYSIA_ACTOR, PROJECT_MANAGER_ACTOR, SESSION_ACTOR_PREFIX};
use crate::models::promise::{self, Promise, PromiseState, Work};
use crate::models::timeline_record::{self, Attribution, TimelineRecord};
use crate::models::user::PROJECT_MANAGER_USER_ID;
use crate::models::work_batch::worker as batch_worker;
use crate::models::work_batch::{self, BatchState, WorkBatch};
use crate::project_managers::batches::{self, BatchError, Submission};
use crate::project_managers::promises::{self, PromiseError};
use crate::realtime::Signal;
use crate::tools::work::manager_changes;
use crate::worker_pool::actions::announce;

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
        on yet. A running batch's worker may wait on a question only you or the person can \
        answer; manager_answer_batch answers it. manager_promises lists what the person promised to tell people once the \
        project's work is done; manager_tie_promise ties a promise to the batch or action item \
        that keeps it, and manager_draft_follow_up drafts its follow-up, in the person's voice, \
        once that work is done. A follow-up is never sent until the person sends it. \
        manager_make_changes changes the project's action items (create, update, resolve, \
        dismiss, comment, move between initiatives) and files an item's own issue in the \
        project's tracker, each change naming the record it came from; the turn's prompt says \
        whether your changes apply at once or wait for a person.",
    ),
    tools: &TOOLS,
};

const TOOLS: [Tool; 10] = [
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
    Tool {
        name: "manager_promises",
        description: "Lists the promises the person made on this project, newest first: each \
            one's id, who it is for, what was promised, the message it was made in, the work it \
            waits on, and where it stands: waiting for its work, due once the work is done, \
            drafted once you drafted its follow-up, kept once the person sent it, or dismissed. \
            By default every state; pass states for some.",
        input_schema: promises_schema,
        run: |context, scope, arguments| run_promises(context, scope, arguments).boxed(),
    },
    Tool {
        name: "manager_tie_promise",
        description: "Ties a waiting promise to the work that keeps it: one of this project's \
            batches (batchId), kept when it succeeds, or one of its action items (actionItemId), \
            kept when it is resolved, which a linked issue closing or pull request merging does. \
            Work already done makes the promise due at once. Answers the promise.",
        input_schema: tie_schema,
        run: |context, scope, arguments| run_tie(context, scope, arguments).boxed(),
    },
    Tool {
        name: "manager_draft_follow_up",
        description: "Drafts the follow-up a due promise is owed: the message, written as the \
            person who made the promise, to the person it is for, replying where it was made. \
            It becomes an item in the person's inbox, which they send, edit, or dismiss; nothing \
            is sent until they do. Answers the promise and its item.",
        input_schema: draft_follow_up_schema,
        run: |context, scope, arguments| run_draft_follow_up(context, scope, arguments).boxed(),
    },
    Tool {
        name: "manager_make_changes",
        description: "Changes this project's action items and initiatives, and files an \
            item's own issue in the project's tracker (file-issue), as one changeset: \
            creating, updating, resolving, dismissing, or commenting on items, moving them into \
            or out of initiatives, and creating initiatives. A comment is posted to the item's \
            linked issue, and resolving moves its issues to done. Every change names the \
            timeline record it came from (causeRecordId). A change can act on what an earlier \
            one creates by naming {\"operation\": n}. When this project lets you write, it \
            applies at once and answers how each change ended; otherwise it waits for a \
            person's approval and answers the staged changeset.",
        input_schema: manager_changes::schema,
        run: |context, scope, arguments| manager_changes::run(context, scope, arguments).boxed(),
    },
    Tool {
        name: "manager_answer_batch",
        description: "Answers the question a running batch's worker waits on, which \
            manager_batches shows as waitingOn question with its waitingDetail. The worker's \
            next turn starts at once with your answer, word for word. Answer only what you know; \
            when the person has to decide, ask them, and answer once they have.",
        input_schema: answer_schema,
        run: |context, scope, arguments| run_answer(context, scope, arguments).boxed(),
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

/// How many promises `manager_promises` lists.
const PROMISES_MAX: i64 = 100;

/// How much of the message a promise was made in `manager_promises` shows, in characters.
const PROMISE_EXCERPT_CHARACTERS: usize = 600;

fn id_property(description: &str) -> Value {
    json!({ "type": "string", "format": "uuid", "description": description })
}

fn answer_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "batchId": id_property("The running batch whose worker asked, from manager_batches."),
            "answer": {
                "type": "string",
                "minLength": 1,
                "maxLength": ANSWER_MAX_CHARACTERS,
                "description": "The answer, as the worker will read it.",
            },
        },
        "required": ["batchId", "answer"],
        "additionalProperties": false,
    })
}

/// The longest answer to a worker's question, in characters.
const ANSWER_MAX_CHARACTERS: usize = 8_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AnswerArguments {
    batch_id: Uuid,
    answer: String,
}

async fn run_answer(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let manager = scope.project_manager()?;
    let arguments: AnswerArguments = parse_arguments(arguments)?;
    let answer = arguments.answer.trim();
    if answer.is_empty() || answer.chars().count() > ANSWER_MAX_CHARACTERS {
        return Err(ToolError::Invalid(format!(
            "answer must hold text, at most {ANSWER_MAX_CHARACTERS} characters"
        )));
    }
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let answered = batch_worker::answer(
        &mut connection,
        arguments.batch_id,
        manager.project_id,
        answer,
        PROJECT_MANAGER_USER_ID,
    )
    .await
    .map_err(|error| match error {
        batch_worker::AnswerError::Refused(reason) => ToolError::Invalid(reason),
        batch_worker::AnswerError::Database(error) => {
            internal(scope, "manager.answer_batch", &error)
        }
    })?;
    announce(&context.events, &answered);
    // The waiting worker listens for the pool's signal and starts its next turn.
    context.events.signal(&Signal::WorkerPool);
    Ok(json!({ "batch": batch_view(&answered) }))
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

fn promises_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "states": {
                "type": "array",
                "items": {
                    "type": "string",
                    "enum": ["waiting", "due", "drafted", "kept", "dismissed"],
                },
                "minItems": 1,
                "uniqueItems": true,
                "description": "Only promises in these states. Omit for every state.",
            },
        },
        "additionalProperties": false,
    })
}

fn tie_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "promiseId": id_property("The promise, from manager_promises."),
            "batchId": id_property("The batch that keeps it, from manager_batches."),
            "actionItemId": id_property("The action item that keeps it, from work_items."),
        },
        "required": ["promiseId"],
        "additionalProperties": false,
    })
}

fn draft_follow_up_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "promiseId": id_property("The due promise, from manager_promises."),
            "message": {
                "type": "string",
                "minLength": 1,
                "maxLength": 20000,
                "description": "The follow-up, as the person who made the promise would write \
                    it to the person it is for: first person, in their tone, saying what was \
                    done and anything they need to know, with nothing that says Elysium wrote it.",
            },
        },
        "required": ["promiseId", "message"],
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

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PromisesArguments {
    #[serde(default)]
    states: Vec<PromiseState>,
}

#[expect(
    clippy::struct_field_names,
    reason = "each names an id, as the tool's input schema does"
)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TieArguments {
    promise_id: Uuid,
    batch_id: Option<Uuid>,
    action_item_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DraftFollowUpArguments {
    promise_id: Uuid,
    message: String,
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

async fn run_promises(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let manager = scope.project_manager()?;
    let arguments: PromisesArguments = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let listed = promise::list_for_project(
        &mut connection,
        manager.project_id,
        &arguments.states,
        PROMISES_MAX,
    )
    .await
    .map_err(|error| internal(scope, "manager.promises", &error))?;
    let record_ids: Vec<Uuid> = listed.iter().map(|made| made.timeline_record_id).collect();
    let records: HashMap<Uuid, TimelineRecord> =
        timeline_record::find_many(&mut connection, &record_ids)
            .await
            .map_err(|error| internal(scope, "manager.promises", &error))?
            .into_iter()
            .map(|record| (record.id, record))
            .collect();
    let promises: Vec<Value> = listed
        .iter()
        .map(|made| promise_view(made, records.get(&made.timeline_record_id)))
        .collect();
    Ok(json!({ "promises": promises }))
}

async fn run_tie(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let manager = scope.project_manager()?;
    let arguments: TieArguments = parse_arguments(arguments)?;
    let work = match (arguments.batch_id, arguments.action_item_id) {
        (Some(batch_id), None) => Work::Batch(batch_id),
        (None, Some(item_id)) => Work::Item(item_id),
        _ => {
            return Err(ToolError::Invalid(
                "name the work that keeps the promise: batchId or actionItemId, one of them"
                    .to_owned(),
            ));
        }
    };
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let tied = promises::tie(
        &mut connection,
        &context.events,
        manager.project_id,
        arguments.promise_id,
        work,
    )
    .await
    .map_err(|error| promise_refused_or_internal(scope, "manager.tie_promise", error))?;
    Ok(json!({ "promise": promise_view(&tied, None) }))
}

async fn run_draft_follow_up(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let manager = scope.project_manager()?;
    let arguments: DraftFollowUpArguments = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let follow_up = promises::draft_follow_up(
        &mut connection,
        &context.events,
        manager.project_id,
        arguments.promise_id,
        &arguments.message,
    )
    .await
    .map_err(|error| promise_refused_or_internal(scope, "manager.draft_follow_up", error))?;
    Ok(json!({
        "promise": promise_view(&follow_up.promise, None),
        "actionItem": { "id": follow_up.item.id, "title": follow_up.item.title },
        "message": "The follow-up waits in the person's inbox; nothing is sent until they send it.",
    }))
}

fn promise_refused_or_internal(
    scope: &CallScope,
    operation: &'static str,
    error: PromiseError,
) -> ToolError {
    match error {
        PromiseError::Refused(reason) => ToolError::Invalid(reason),
        PromiseError::Internal(error) => internal(scope, operation, &error),
    }
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
        actor if actor.starts_with(SESSION_ACTOR_PREFIX) => "a coding session",
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

/// A promise as the tools show it, with the message it was made in when it was read.
fn promise_view(made: &Promise, record: Option<&TimelineRecord>) -> Value {
    let made_in = record.map(|record| {
        let said: String = record
            .body
            .chars()
            .take(PROMISE_EXCERPT_CHARACTERS)
            .collect();
        json!({ "recordId": record.id, "title": record.title, "said": said })
    });
    json!({
        "id": made.id,
        "state": made.state,
        "recipient": made.recipient,
        "promise": made.summary,
        "madeIn": made_in,
        "recordId": made.timeline_record_id,
        "batchId": made.work_batch_id,
        "actionItemId": made.work_item_id,
        "dueAt": made.due_at,
        "followUpItemId": made.follow_up_item_id,
        "foundAt": made.created_at,
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
        "waitingOn": batch.waiting_on,
        "waitingDetail": batch.waiting_detail,
        "waitingUntil": batch.waiting_until,
    })
}
