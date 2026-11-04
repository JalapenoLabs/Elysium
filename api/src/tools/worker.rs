// Copyright © 2026 Jalapeno Labs

//! `elysium_worker`: a worker's own tools, for the one batch it runs.
//!
//! A worker reads its batch with `worker_batch`, and says how each turn ends with one of two
//! tools: `worker_finish` once the batch is done or cannot be done, or `worker_wait` while it
//! waits on a review, CI, or an answer, keeping its container. The API answers these calls and
//! holds no claim on the batch's slot, so each only files a report on the batch
//! (`crate::models::work_batch::worker`); the engine acts on it once the turn is over. Every call
//! is checked against the batch as the database holds it now. See `docs/engine.md`, Workers.

use chrono::{TimeDelta, Utc};
use futures_util::FutureExt as _;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    CallScope, Instructions, Tool, ToolContext, ToolError, ToolServer, internal, parse_arguments,
};
use crate::models::work_batch::worker::{self, Report, WAIT_DETAIL_MAX_CHARACTERS, Wait};
use crate::models::work_batch::{self, RESULT_MAX_CHARACTERS};

pub const SERVER: ToolServer = ToolServer {
    name: "elysium_worker",
    instructions: Instructions::Fixed(
        "Your work batch, and how you report on it. worker_batch reads the batch: its goal, \
        the repositories it needs, the action items it works on, and any answers your Project \
        Manager gave. End every turn with exactly one of the two reports. worker_finish says the \
        batch is done, or that it cannot be done and why: what you did, the pull requests you \
        opened, and anything left. worker_wait says you are waiting on a review of your pull \
        request, a CI run, or an answer to a question only a person or your Project Manager can \
        give; your container is kept, and your next turn starts when you said to look again, or \
        at once when your question is answered. Call the report last, then end your turn.",
    ),
    tools: &TOOLS,
};

const TOOLS: [Tool; 3] = [
    Tool {
        name: "worker_batch",
        description: "Reads your work batch: its title, goal, repositories, the action items it \
            works on, what you wait on now, and the answers your Project Manager gave since you \
            last began to wait.",
        input_schema: no_arguments_schema,
        run: |context, scope, arguments| run_batch(context, scope, arguments).boxed(),
    },
    Tool {
        name: "worker_wait",
        description: "Reports that your turn ends waiting, keeping your container: on a review \
            of your pull request, a CI run, or the answer to a question. detail says what you \
            wait for, in a sentence a person can act on; a question is put to your Project \
            Manager word for word. checkBackInMinutes is when to look again (5 to 1440); by \
            default 15 for CI, 60 for a review, and 240 for a question, whose answer starts \
            your next turn at once.",
        input_schema: wait_schema,
        run: |context, scope, arguments| run_wait(context, scope, arguments).boxed(),
    },
    Tool {
        name: "worker_finish",
        description: "Reports that your batch is over: succeeded true when its goal is met, \
            false when it cannot be met. result says what you did, the pull requests you opened, \
            and anything left for someone else, as its Project Manager will read it.",
        input_schema: finish_schema,
        run: |context, scope, arguments| run_finish(context, scope, arguments).boxed(),
    },
];

/// The bounds on when a worker looks again, in minutes: often enough for a quick CI run, and at
/// least once a day for a review or a question nobody has answered.
const CHECK_BACK_MINUTES: (i64, i64) = (5, 1_440);

/// When a worker looks again by default, by what it waits on.
const fn default_check_back_minutes(on: Wait) -> i64 {
    match on {
        Wait::Ci => 15,
        Wait::Review => 60,
        Wait::Question => 240,
    }
}

fn no_arguments_schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn wait_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "on": { "type": "string", "enum": ["review", "ci", "question"] },
            "detail": { "type": "string", "minLength": 1, "maxLength": WAIT_DETAIL_MAX_CHARACTERS },
            "checkBackInMinutes": {
                "type": "integer",
                "minimum": CHECK_BACK_MINUTES.0,
                "maximum": CHECK_BACK_MINUTES.1,
            },
        },
        "required": ["on", "detail"],
        "additionalProperties": false,
    })
}

fn finish_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "succeeded": { "type": "boolean" },
            "result": { "type": "string", "minLength": 1, "maxLength": RESULT_MAX_CHARACTERS },
        },
        "required": ["succeeded", "result"],
        "additionalProperties": false,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WaitArguments {
    on: Wait,
    detail: String,
    check_back_in_minutes: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FinishArguments {
    succeeded: bool,
    result: String,
}

async fn run_batch(
    context: &ToolContext,
    scope: &CallScope,
    _arguments: &str,
) -> Result<Value, ToolError> {
    let worker = scope.worker()?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let batch = work_batch::find(&mut connection, worker.batch_id)
        .await
        .map_err(|error| internal(scope, "worker.batch", &error))?;
    let answers = worker::answers_since_waiting(&mut connection, batch.id)
        .await
        .map_err(|error| internal(scope, "worker.answers", &error))?;
    let waiting = batch.waiting_on.map(
        |on| json!({ "on": on, "detail": batch.waiting_detail, "until": batch.waiting_until }),
    );
    Ok(json!({
        "batch": {
            "id": batch.id,
            "title": batch.title,
            "goal": batch.goal,
            "repositories": batch.repositories(),
            "actionItemIds": batch.action_items(),
            "waiting": waiting,
            "answers": answers,
        }
    }))
}

async fn run_wait(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: WaitArguments = parse_arguments(arguments)?;
    let detail = arguments.detail.trim();
    if detail.is_empty() || detail.chars().count() > WAIT_DETAIL_MAX_CHARACTERS {
        return Err(ToolError::Invalid(format!(
            "detail must hold text, at most {WAIT_DETAIL_MAX_CHARACTERS} characters"
        )));
    }
    let minutes = arguments
        .check_back_in_minutes
        .unwrap_or_else(|| default_check_back_minutes(arguments.on));
    if !(CHECK_BACK_MINUTES.0..=CHECK_BACK_MINUTES.1).contains(&minutes) {
        return Err(ToolError::Invalid(format!(
            "checkBackInMinutes must be {} to {}",
            CHECK_BACK_MINUTES.0, CHECK_BACK_MINUTES.1
        )));
    }
    let until = Utc::now() + TimeDelta::minutes(minutes);
    let report = Report::Waiting {
        on: arguments.on,
        detail: detail.to_owned(),
        until,
    };
    file(context, scope, &report).await?;
    Ok(json!({
        "reported": "waiting",
        "until": until,
        "next": "End your turn now. Your container and everything in it are kept; your next \
            turn resumes this session.",
    }))
}

async fn run_finish(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: FinishArguments = parse_arguments(arguments)?;
    let result = arguments.result.trim();
    if result.is_empty() || result.chars().count() > RESULT_MAX_CHARACTERS {
        return Err(ToolError::Invalid(format!(
            "result must hold text, at most {RESULT_MAX_CHARACTERS} characters"
        )));
    }
    let report = Report::Finished {
        succeeded: arguments.succeeded,
        result: result.to_owned(),
    };
    file(context, scope, &report).await?;
    Ok(json!({
        "reported": "finished",
        "next": "End your turn now. Your container is removed once the turn is over.",
    }))
}

/// Files `report` for the caller's batch, which must still be running.
async fn file(context: &ToolContext, scope: &CallScope, report: &Report) -> Result<(), ToolError> {
    let worker = scope.worker()?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let filed = worker::report(&mut connection, worker.batch_id, worker.project_id, report)
        .await
        .map_err(|error| internal(scope, "worker.report", &error))?;
    if !filed {
        return Err(ToolError::Invalid(format!(
            "batch {} is no longer running, so there is nothing to report on; end your turn",
            worker.batch_id
        )));
    }
    tracing::event!(
        name: "tools.worker.reported",
        tracing::Level::INFO,
        tool.caller = scope.label(),
        work_batch.id = %worker.batch_id,
        "a worker filed how its turn ends",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_wait_looks_again_within_a_day() {
        for on in [Wait::Ci, Wait::Review, Wait::Question] {
            let minutes = default_check_back_minutes(on);
            assert!((CHECK_BACK_MINUTES.0..=CHECK_BACK_MINUTES.1).contains(&minutes));
        }
    }

    #[test]
    fn the_wait_schema_names_every_kind_the_model_reads() {
        let schema = wait_schema();
        let kinds = schema["properties"]["on"]["enum"]
            .as_array()
            .expect("an enum");
        for kind in kinds {
            let parsed: Wait =
                serde_json::from_value(kind.clone()).expect("every listed kind reads");
            assert_eq!(json!(parsed), *kind);
        }
    }
}
