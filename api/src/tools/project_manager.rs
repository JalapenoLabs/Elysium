// Copyright © 2026 Jalapeno Labs

//! `elysium_project_manager`: a Project Manager's own tools, for its one project.
//!
//! It reads its project's timeline, every owner's records on it, its batches, and the knowledge
//! base pages linked to the project, and it submits and withdraws batches for the worker pool.
//! Work nobody asked for that it notices it reports as a finding (`crate::findings`), for a
//! person to accept or dismiss, rather than submitting or filing it. It also keeps its project's
//! promises: it lists them, ties each to the work that keeps it, and
//! drafts a follow-up once that work is done, through
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
use crate::findings::{self, FindingError, Report};
use crate::meetings::attachments;
use crate::models::attribution::{ELYSIA_ACTOR, PROJECT_MANAGER_ACTOR, SESSION_ACTOR_PREFIX};
use crate::models::finding::Reporter;
use crate::models::page_project;
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
        "Your project's timeline, its docs, and its work batches. manager_timeline lists what is \
        on the project's timeline, newest first: meeting topics, messages, mail, your own turns \
        and batches. manager_record reads one record in full. manager_pages lists the knowledge \
        base pages linked to the project, its docs, and manager_page reads one. manager_batches \
        lists the batches you submitted and where each stands; check it before submitting, so \
        no work is submitted twice. manager_submit_batch proposes a piece of work for a worker: \
        write its goal so a worker who has seen nothing else can finish it, name the goal in the \
        project's brief it serves, and name the records and action items it came from. A batch \
        waits for a person to approve it, edit and approve it, or reject it with a reason, which \
        wakes you, unless the project's level of autonomy, which your prompt names, lets a \
        human-rooted one queue at once; a found batch always waits. Queued, it waits in the one \
        queue for the whole workspace, which a person or Elysia may reorder or pause. \
        manager_cancel_batch withdraws a batch no worker has started on yet. Work nobody asked \
        for that you notice is a finding, not a batch: manager_report_finding puts it on the \
        project's Findings list for a person to accept or dismiss, and you never submit a batch, \
        create an item, or file an issue for it. A running batch's \
        worker may wait on a question only you or the person can answer; manager_answer_batch \
        answers it. manager_promises lists what the person promised to tell people once the \
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

const TOOLS: [Tool; 13] = [
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
        description: "Reads one record on this project's timeline in full, with the files and \
            frames pinned in it when it is a meeting's excerpt, each opened with \
            meeting_attachment_get.",
        input_schema: record_schema,
        run: |context, scope, arguments| run_record(context, scope, arguments).boxed(),
    },
    Tool {
        name: "manager_pages",
        description: "Lists the knowledge base pages linked to this project, its docs, newest \
            published first: each one's id, title, version, and when it was published and \
            linked. A page linked since your last turn is here.",
        input_schema: pages_schema,
        run: |context, scope, arguments| run_pages(context, scope, arguments).boxed(),
    },
    Tool {
        name: "manager_page",
        description: "Reads one page linked to this project: its title, version, and text, \
            which is Markdown, with a learning's category and evidence. A long page answers in \
            parts: pass offset as the nextOffset it gave to read on.",
        input_schema: page_schema,
        run: |context, scope, arguments| run_page(context, scope, arguments).boxed(),
    },
    Tool {
        name: "manager_batches",
        description: "Lists this project's work batches, newest first: each one's id, title, \
            priority, when it is scheduled, where it stands (proposed and waiting for a person's \
            approval, queued for a slot, paused by a person or Elysia, assigned a slot and waiting \
            for its worker, running, succeeded, failed, cancelled, or rejected by a person), and \
            what its worker reported or why it was cancelled or rejected. By default every state; \
            pass states for some.",
        input_schema: batches_schema,
        run: |context, scope, arguments| run_batches(context, scope, arguments).boxed(),
    },
    Tool {
        name: "manager_submit_batch",
        description: "Proposes a work batch for a worker: a title, a goal complete enough to \
            work from alone, the goal in the project's brief it serves (serves, by its heading), \
            the repositories it needs, and a priority, asap, normal, or scheduled with \
            scheduledFor (an ISO 8601 time with its offset). recordIds, actionItemIds, and \
            messageIds name what called for it, which the person reads as they decide; a batch \
            naming none is refused, and so is work that serves no goal in the brief. Its origin \
            (human_rooted when anything named traces to a person, found when all of it is an \
            agent's) and its generation (how many batches led to it) are worked out from them and \
            from what woke this turn, and one past the generation limit is refused, saying why. \
            It is put on the project's timeline. A found batch waits, proposed, until a person \
            approves it; a human-rooted one waits too unless the project's level of autonomy lets \
            it queue at once, as your prompt says. Answers the stored batch, with its state.",
        input_schema: submit_schema,
        run: |context, scope, arguments| run_submit(context, scope, arguments).boxed(),
    },
    Tool {
        name: "manager_report_finding",
        description: "Reports work you noticed that nobody asked for and the project may need, \
            such as a gap a meeting or a worker's result shows: title says what in a line; detail \
            says what you saw, why it matters, and what acting on it involves, so a worker who \
            has seen nothing else could do it; repositoryUrl and location say where (a path \
            without line numbers, a URL, a page, or a place's name); cost is small (an hour or \
            less), medium (about a batch), or large (more than one). It goes on the project's \
            Findings list for a person to accept, which files its issue and submits its batch, \
            proposed or queued as the project's level of autonomy says, or dismiss, and answers \
            if the same finding was reported before and what a person made of it. Never submit \
            a batch, create an item, or file an issue for it yourself.",
        input_schema: findings::report_schema,
        run: |context, scope, arguments| run_report_finding(context, scope, arguments).boxed(),
    },
    Tool {
        name: "manager_cancel_batch",
        description: "Withdraws one of this project's batches that no worker has started on \
            yet (proposed, queued, paused, or assigned), saying why.",
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

/// How much of a page's text `manager_page` answers at once, in characters. A page holds up to
/// 100,000, more than one answer should, so a longer one reads in parts.
const PAGE_PART_CHARACTERS: usize = 40_000;

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

fn pages_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false,
    })
}

fn page_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pageId": id_property("The page's id, from your prompt or manager_pages."),
            "offset": {
                "type": "integer",
                "minimum": 0,
                "description": "Where to read on from: the nextOffset the last part answered.",
            },
        },
        "required": ["pageId"],
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
                        "proposed", "queued", "paused", "assigned", "running", "succeeded",
                        "failed", "cancelled", "rejected",
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
            "serves": {
                "type": "string",
                "minLength": 1,
                "maxLength": 300,
                "description": "The goal in the project's brief this work serves, by its heading \
                    as your prompt lists it. A batch that serves none is refused: ask the person \
                    whether the work belongs instead.",
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
            "messageIds": ids("The messages sent to you that called for it, by the ids your \
                prompt gives them."),
        },
        "required": ["title", "goal", "serves", "priority"],
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PageArguments {
    page_id: Uuid,
    #[serde(default)]
    offset: usize,
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
    let pinned = attachments::pinned_in(&mut connection, record)
        .await
        .map_err(|error| internal(scope, "manager.record.pinned", &error))?;
    view["pinned"] = pinned.iter().map(attachments::agent_view).collect();
    Ok(json!({ "record": view }))
}

async fn run_pages(
    context: &ToolContext,
    scope: &CallScope,
    _arguments: &str,
) -> Result<Value, ToolError> {
    let manager = scope.project_manager()?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let pages = page_project::pages_of(&mut connection, manager.project_id)
        .await
        .map_err(|error| internal(scope, "manager.pages", &error))?;
    let listed: Vec<Value> = pages
        .into_iter()
        .map(|linked| {
            json!({
                "id": linked.page.id,
                "title": linked.page.title,
                "version": linked.page.current_version,
                "publishedAt": linked.published_at,
                "linkedAt": linked.linked_at,
            })
        })
        .collect();
    Ok(json!({ "pages": listed }))
}

async fn run_page(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let manager = scope.project_manager()?;
    let arguments: PageArguments = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let linked = page_project::linked_page(&mut connection, manager.project_id, arguments.page_id)
        .await
        .map_err(|error| internal(scope, "manager.page", &error))?;
    let Some(page) = linked else {
        return Err(ToolError::NotFound(format!(
            "page {} is not linked to this project; call manager_pages for the ones that are",
            arguments.page_id
        )));
    };
    let (text, next_offset) = page_part(&page.body, arguments.offset);
    Ok(json!({
        "page": {
            "id": page.id,
            "title": page.title,
            "version": page.current_version,
            "category": page.category,
            "evidence": page.evidence,
            "text": text,
            "nextOffset": next_offset,
        },
    }))
}

/// One part of a page's `body` from `offset`, in characters, and where the next begins, if any.
fn page_part(body: &str, offset: usize) -> (String, Option<usize>) {
    let mut rest = body.chars().skip(offset);
    let part: String = rest.by_ref().take(PAGE_PART_CHARACTERS).collect();
    let next_offset = rest
        .next()
        .is_some()
        .then_some(offset + PAGE_PART_CHARACTERS);
    (part, next_offset)
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
    Ok(json!({
        "batch": batch_view(&batch),
        "note": submitted_note(batch.state),
    }))
}

/// What a Project Manager is told about the batch it just submitted, following the state it was
/// stored in, which the project's level of autonomy decided, so it never reports queued work as
/// waiting for a person.
const fn submitted_note(state: BatchState) -> &'static str {
    match state {
        BatchState::Queued => {
            "Queued: the project's level of autonomy lets work a person asked for run without \
                an approval, so it waits only for a worker's slot."
        }
        _ => {
            "Proposed: it waits for a person to approve it before any worker starts on it. A \
                rejection wakes you with their reason."
        }
    }
}

async fn run_report_finding(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let manager = scope.project_manager()?;
    let report: Report = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let reported = findings::report(
        &mut connection,
        &context.events,
        manager.project_id,
        Reporter::ProjectManager {
            turn_id: manager.turn_id,
        },
        report,
    )
    .await
    .map_err(|error| match error {
        FindingError::Invalid(reason) => ToolError::Invalid(reason),
        error => internal(scope, "manager.report_finding", &error),
    })?;
    Ok(findings::answer(&reported))
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
        "serves": batch.serves,
        "repositories": batch.repositories,
        "priority": batch.priority,
        "scheduledFor": batch.scheduled_for,
        "state": batch.state,
        "result": batch.result,
        "recordIds": batch.source_record_ids,
        "actionItemIds": batch.action_item_ids,
        "messageIds": batch.source_message_ids,
        "origin": batch.origin,
        "generation": batch.generation,
        "parentBatchIds": batch.parent_batch_ids,
        "submittedAt": batch.created_at,
        "startedAt": batch.started_at,
        "finishedAt": batch.finished_at,
        "waitingOn": batch.waiting_on,
        "waitingDetail": batch.waiting_detail,
        "waitingUntil": batch.waiting_until,
    })
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use secrecy::ExposeSecret;
    use serde_json::{Value, json};
    use tower::ServiceExt as _;
    use uuid::Uuid;

    use super::{PAGE_PART_CHARACTERS, SERVER, page_part, submitted_note};
    use crate::models::agent::Author;
    use crate::models::page::{self, PageEdit};
    use crate::models::page_project;
    use crate::models::project::{self, NewProject};
    use crate::models::work_batch::BatchState;
    use crate::routes;
    use crate::runner::McpServer;
    use crate::state::AppState;
    use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database};
    use crate::tools::{CallScope, ProjectManagerScope};

    /// What a turn's agent reads from calling `tool` with `arguments`: whether it failed, and the
    /// text.
    async fn call(state: &AppState, bearer: &str, tool: &str, arguments: Value) -> (bool, String) {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": tool, "arguments": arguments },
        });
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/agent-tools/{}", SERVER.name))
            .header(header::HOST, "api:8080")
            .header(header::AUTHORIZATION, bearer)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .body(Body::from(body.to_string()))
            .expect("a request builds");
        let response = routes::router(state)
            .with_state(state.clone())
            .oneshot(request)
            .await
            .expect("the router answers");
        assert_eq!(response.status(), StatusCode::OK, "{tool}");
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
            .await
            .expect("the body reads");
        let answer: Value = serde_json::from_slice(&bytes).expect("the server answers JSON");
        let result = &answer["result"];
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{tool} answers text: {answer}"))
            .to_owned();
        (result["isError"] == true, text)
    }

    /// The JSON a call that succeeded answered.
    fn answered((failed, text): (bool, String)) -> Value {
        assert!(!failed, "{text}");
        serde_json::from_str(&text).expect("a success is JSON")
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
    async fn a_project_manager_reads_its_projects_docs_in_parts_and_no_other_page() {
        let (url, mut connection) = migrated_database().await;
        let state = app_state(&url).await;
        let project_id = project::create(
            &mut connection,
            &NewProject {
                name: "Launch".to_owned(),
                description: String::new(),
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("a project")
        .id;
        let author = Author {
            user_id: TEST_PERSON_ID,
            agent: None,
            at: chrono::Utc::now(),
        };
        let long_body = "a".repeat(PAGE_PART_CHARACTERS + 5);
        let mut page_ids = Vec::new();
        for (title, body) in [
            ("Launch checklist", long_body.as_str()),
            ("Someone else's page", "Private to its project."),
        ] {
            let edit = PageEdit {
                title: title.to_owned(),
                body: body.to_owned(),
                learning: None,
            };
            let created = page::create(&mut connection, None, edit, author)
                .await
                .expect("a page");
            page_ids.push(created.page.id);
        }
        let (linked, unlinked) = (page_ids[0], page_ids[1]);
        page_project::link(&mut connection, linked, project_id, author)
            .await
            .expect("linked");

        let scope = CallScope::ProjectManager(ProjectManagerScope {
            project_id,
            turn_id: Uuid::now_v7(),
        });
        let grant = state
            .grants
            .grant(&scope, &[&SERVER])
            .await
            .expect("a grant is stored");
        let McpServer::Http { headers, .. } = &grant.mcp_servers()[0] else {
            panic!("tools are served over HTTP");
        };
        let bearer = headers[0].1.expose_secret().to_owned();

        // Its brief is one of its docs too, linked since the project was created.
        let listed = answered(call(&state, &bearer, "manager_pages", json!({})).await);
        assert_eq!(listed["pages"].as_array().expect("pages").len(), 2);
        assert_eq!(listed["pages"][0]["title"], "Launch checklist");
        assert_eq!(listed["pages"][1]["title"], "Launch brief");

        let arguments = json!({ "pageId": linked });
        let first = answered(call(&state, &bearer, "manager_page", arguments).await);
        assert_eq!(first["page"]["nextOffset"], PAGE_PART_CHARACTERS);

        let arguments = json!({ "pageId": linked, "offset": PAGE_PART_CHARACTERS });
        let rest = answered(call(&state, &bearer, "manager_page", arguments).await);
        assert_eq!(rest["page"]["text"], "aaaaa");
        assert_eq!(rest["page"]["nextOffset"], Value::Null);

        let arguments = json!({ "pageId": unlinked });
        let (failed, text) = call(&state, &bearer, "manager_page", arguments).await;
        assert!(failed, "a page off the project is refused: {text}");
        assert!(text.contains("not linked to this project"), "{text}");
        assert!(!text.contains("Private to its project."), "{text}");
    }

    #[test]
    fn a_submitted_batch_is_described_as_the_state_it_was_stored_in() {
        assert!(submitted_note(BatchState::Queued).starts_with("Queued:"));
        assert!(submitted_note(BatchState::Proposed).starts_with("Proposed:"));
        assert!(!submitted_note(BatchState::Queued).contains("approve it"));
    }

    #[test]
    fn a_short_page_answers_whole() {
        assert_eq!(page_part("Short.", 0), ("Short.".to_owned(), None));
    }

    #[test]
    fn a_long_page_reads_in_parts_that_add_up_to_it() {
        // Multibyte, so a part is cut between characters and never inside one.
        let body = "é".repeat(PAGE_PART_CHARACTERS + 3);

        let (first, next_offset) = page_part(&body, 0);
        assert_eq!(first.chars().count(), PAGE_PART_CHARACTERS);
        assert_eq!(next_offset, Some(PAGE_PART_CHARACTERS));

        let (rest, next_offset) = page_part(&body, PAGE_PART_CHARACTERS);
        assert_eq!(rest, "ééé");
        assert_eq!(next_offset, None);
    }

    #[test]
    fn a_page_exactly_one_part_long_has_no_next_part() {
        let body = "a".repeat(PAGE_PART_CHARACTERS);
        assert_eq!(page_part(&body, 0).1, None);
    }

    #[test]
    fn an_offset_past_the_end_reads_nothing() {
        assert_eq!(page_part("Short.", 100), (String::new(), None));
    }
}
