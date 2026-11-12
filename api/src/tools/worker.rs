// Copyright © 2026 Jalapeno Labs

//! `elysium_worker`: a worker's own tools, for the one batch it runs.
//!
//! A worker reads its batch with `worker_batch`, records each pull request it opens with
//! `worker_pull_request`, reports work it noticed beyond its batch with `worker_report_finding`
//! rather than doing or filing it (`crate::findings`), and says how each turn ends with one of
//! two tools: `worker_finish` once the batch is done or cannot be done, or `worker_wait` while it
//! waits on a review, CI, or an answer, keeping its container. The API answers these calls and
//! holds no claim on the batch's
//! slot, so each only files a report on the batch (`crate::models::work_batch::worker`), or
//! records a pull request (`crate::models::work_batch::pull_request`) and puts why its batch
//! exists in the pull request's description (`crate::worker_pool::description`); the engine acts
//! on them once the turn is over, and only the engine merges (`crate::worker_pool::merging`).
//! Every call is checked against the batch as the database holds it now. See `docs/engine.md`,
//! Workers.

use chrono::{TimeDelta, Utc};
use futures_util::FutureExt as _;
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    CallScope, Instructions, Tool, ToolContext, ToolError, ToolServer, internal, parse_arguments,
};
use crate::findings::{self, FindingError};
use crate::github::issues::IssueRef;
use crate::github::pulls::PullRequest;
use crate::meetings::attachments;
use crate::models::finding::Reporter;
use crate::models::work_batch::pull_request::{self, NewPullRequest, RecordError};
use crate::models::work_batch::why::{self, Why};
use crate::models::work_batch::worker::{self, Report, WAIT_DETAIL_MAX_CHARACTERS, Wait};
use crate::models::work_batch::{self, BatchState, RESULT_MAX_CHARACTERS, WorkBatch};
use crate::models::worker_setup::{self, MergePolicy, WorkerSetup};
use crate::models::{github_credential, project, timeline_record};
use crate::routes::v1::coding_sessions::github_token;
use crate::worker_pool::actions::announce;
use crate::worker_pool::description;
use crate::worker_pool::merging;

pub const SERVER: ToolServer = ToolServer {
    name: "elysium_worker",
    instructions: Instructions::Fixed(
        "Your work batch, and how you report on it. worker_batch reads the batch: its goal, \
        the repositories it needs, the action items it works on, the meeting excerpts it came \
        from with the files and frames pinned in them, the pull requests you recorded, and any \
        answers your Project Manager gave. meeting_attachment_get opens a pinned file or frame. \
        worker_pull_request records a pull request you opened, right after you open it: Elysium merges it in a repository that lets it, once \
        every check passed and no review is left open, and a person merges it everywhere else. \
        worker_report_finding reports work you noticed that your batch does not ask for: a bug, \
        a gap, a cleanup. A person decides whether it is done; never do it, file an issue for \
        it, or open a pull request for it yourself. It ends nothing, so carry on with your batch. \
        End every turn with exactly one of the two reports. worker_finish says the \
        batch is done, or that it cannot be done and why: what you did, the pull requests you \
        opened, and anything left. worker_wait says you are waiting on a review of your pull \
        request, a CI run, or an answer to a question only a person or your Project Manager can \
        give; your container is kept, and your next turn starts when you said to look again, or \
        at once when your question is answered. Call the report last, then end your turn.",
    ),
    tools: &TOOLS,
};

const TOOLS: [Tool; 5] = [
    Tool {
        name: "worker_batch",
        description: "Reads your work batch: its title, goal, repositories, the action items it \
            works on, the meeting excerpts it came from with the files and frames pinned in each, \
            the pull requests you recorded and where each stands, what you wait on now, and the \
            answers your Project Manager gave since you last began to wait.",
        input_schema: no_arguments_schema,
        run: |context, scope, arguments| run_batch(context, scope, arguments).boxed(),
    },
    Tool {
        name: "worker_pull_request",
        description: "Records a pull request you opened for this batch, by its github.com URL, \
            right after you open it. It must be yours: opened by the project's GitHub account \
            during this batch, from a branch of the repository itself. Elysium then adds why \
            the batch exists to the end of its description; call it again to put that back if \
            an edit removed it. Answers who merges it: Elysium, once every check passed, no \
            review thread is open, and nobody asked for changes, or a person. Never merge a pull \
            request yourself.",
        input_schema: pull_request_schema,
        run: |context, scope, arguments| run_pull_request(context, scope, arguments).boxed(),
    },
    Tool {
        name: "worker_report_finding",
        description: "Reports work you noticed that your batch does not ask for, such as a bug \
            beside your change, a missing test, or a cleanup: title says what in a line; detail \
            says what you saw, why it matters, and what fixing it involves, so a worker who has \
            seen nothing else could do it; repositoryUrl and location say where (a path without \
            line numbers, a URL, or a place's name); cost is small (an hour or less), medium \
            (about a batch), or large (more than one). It goes on the project's Findings list for \
            a person to accept or dismiss, and answers if the same finding was reported before \
            and what a person made of it. Never do the work, file an issue for it, or open a pull \
            request for it yourself. It is not a report on your batch: end your turn with \
            worker_finish or worker_wait as always.",
        input_schema: findings::report_schema,
        run: |context, scope, arguments| run_report_finding(context, scope, arguments).boxed(),
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

fn pull_request_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "url": { "type": "string", "minLength": 1, "maxLength": 2048 },
        },
        "required": ["url"],
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
struct PullRequestArguments {
    url: String,
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
    let recorded = pull_request::of_batch(&mut connection, batch.id)
        .await
        .map_err(|error| internal(scope, "worker.pull_requests", &error))?;
    let excerpts = excerpts_of(&mut connection, &batch)
        .await
        .map_err(|error| internal(scope, "worker.excerpts", &error))?;
    let waiting = batch.waiting_on.map(
        |on| json!({ "on": on, "detail": batch.waiting_detail, "until": batch.waiting_until }),
    );
    let pull_requests: Vec<Value> = recorded
        .iter()
        .map(|recorded| {
            json!({
                "pullRequest": recorded.reference(),
                "url": recorded.url,
                "state": recorded.state,
                "mergedBy": recorded.merged_by,
                "mergedByElysium": recorded.merged_by_elysium,
                "heldBecause": recorded.held_because,
            })
        })
        .collect();
    Ok(json!({
        "batch": {
            "id": batch.id,
            "title": batch.title,
            "goal": batch.goal,
            "repositories": batch.repositories(),
            "actionItemIds": batch.action_items(),
            "excerpts": excerpts,
            "pullRequests": pull_requests,
            "waiting": waiting,
            "answers": answers,
        }
    }))
}

/// The meeting excerpts `batch` came from, each with the files and frames pinned in it, which
/// travel with an excerpt to the worker its work reaches. Its other records are its Project
/// Manager's to put in its goal.
async fn excerpts_of(
    connection: &mut diesel_async::AsyncPgConnection,
    batch: &WorkBatch,
) -> diesel::QueryResult<Vec<Value>> {
    let records = timeline_record::find_many(connection, &batch.source_records()).await?;
    let mut excerpts = Vec::new();
    for record in records {
        if record.excerpt_mark_id.is_none() {
            continue;
        }
        let pinned = attachments::pinned_in(connection, &record).await?;
        excerpts.push(json!({
            "recordId": record.id,
            "title": record.title,
            "summary": record.metadata["summary"].as_str(),
            "pinned": pinned.iter().map(attachments::agent_view).collect::<Vec<_>>(),
        }));
    }
    Ok(excerpts)
}

async fn run_pull_request(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    const OPERATION: &str = "worker.pull_request";

    let arguments: PullRequestArguments = parse_arguments(arguments)?;
    let reference = IssueRef::from_pull_request_url(&arguments.url).ok_or_else(|| {
        ToolError::Invalid(
            "url is not a GitHub pull request; one reads https://github.com/owner/name/pull/12"
                .to_owned(),
        )
    })?;
    let Recording {
        batch,
        token,
        declared,
    } = recording(context, scope).await?;
    let Some(token) = token else {
        return Err(ToolError::Invalid(
            "this project's workers have no GitHub token, so Elysium cannot read the pull \
             request; name it in your result instead"
                .to_owned(),
        ));
    };
    let found = context
        .github
        .pull_request(&token, &reference)
        .await
        .map_err(|error| ToolError::Provider(error.to_string()))?
        .ok_or_else(|| {
            ToolError::NotFound(format!("the project's GitHub token cannot see {reference}"))
        })?;
    let number = own_number(&found, &batch, &reference)?;

    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let recorded = pull_request::record(
        &mut connection,
        NewPullRequest {
            work_batch_id: batch.id,
            repository: &found.repository,
            number,
            url: &found.url,
            title: &found.title,
        },
    )
    .await
    .map_err(|error| match error {
        RecordError::OtherBatch(other) => ToolError::Invalid(format!(
            "batch {other} recorded {reference}; a pull request is one batch's work"
        )),
        RecordError::Database(error) => internal(scope, OPERATION, &error),
    })?;
    announce(&context.events, &batch);

    // Recorded first, since that is what lets it merge. The chain is read now and the connection
    // given back, so none is held across GitHub's round trips.
    let why = why::read(&mut connection, &batch)
        .await
        .map_err(|error| internal(scope, "worker.pull_request.why", &error))?;
    drop(connection);
    let description = describe(context, scope, &batch, &token, &reference, &why).await;

    let (merged_by, next) = match merging::policy_for(&declared, &recorded.github_url()) {
        MergePolicy::Auto => (
            "elysium",
            "Elysium looks at it each time a wait of yours ends, and before your batch finishes: \
             it merges it once every check passed, no review thread is open, and nobody asked \
             for changes, and tells you what holds it otherwise. Address every review comment, \
             then report waiting on ci or review.",
        ),
        MergePolicy::Person => (
            "person",
            "A person merges pull requests in this repository. Address every review comment, \
             and report waiting on review until they merge it.",
        ),
    };
    Ok(json!({
        "pullRequest": {
            "pullRequest": recorded.reference(),
            "url": recorded.url,
            "title": recorded.title,
            "state": recorded.state,
        },
        "mergedBy": merged_by,
        "description": description,
        "next": next,
    }))
}

/// Puts why `batch` exists, `why`, in the description of the pull request `reference` names, and
/// answers what its worker is told of it.
///
/// A description GitHub would not take is said in the answer and logged rather than failing the
/// call: the pull request is recorded already, which is what lets it merge, and recording it
/// again tries the description again.
async fn describe(
    context: &ToolContext,
    scope: &CallScope,
    batch: &WorkBatch,
    token: &SecretString,
    reference: &IssueRef,
    why: &Why,
) -> String {
    let written =
        description::write(&context.github, token, reference, why, &context.public_url).await;
    if let Err(error) = written {
        tracing::event!(
            name: "tools.worker.description_unwritten",
            tracing::Level::WARN,
            tool.caller = scope.label(),
            work_batch.id = %batch.id,
            pull_request = %reference,
            error.message = %error,
            "could not put why a batch exists in its pull request's description",
        );
        return format!(
            "It is recorded, but Elysium could not add why this batch exists to its \
             description: {error}. Call worker_pull_request again to try once more."
        );
    }
    format!(
        "Its description ends with why this batch exists, between {} and {}. Keep that section \
         when you edit the description; calling worker_pull_request again puts it back.",
        description::START,
        description::END
    )
}

async fn run_report_finding(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    const OPERATION: &str = "worker.report_finding";

    let worker = scope.worker()?;
    let report: findings::Report = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let batch = work_batch::find(&mut connection, worker.batch_id)
        .await
        .map_err(|error| internal(scope, OPERATION, &error))?;
    // The grant names the batch's own project, so this holds unless a grant was written wrong;
    // a finding must never land on another project's list either way.
    if batch.project_id != worker.project_id {
        return Err(ToolError::Invalid(format!(
            "batch {} is not this project's, so it reports nothing here",
            worker.batch_id
        )));
    }
    if batch.state != BatchState::Running {
        return Err(ToolError::Invalid(format!(
            "batch {} is no longer running, so it reports nothing; end your turn",
            worker.batch_id
        )));
    }
    let reported = findings::report(
        &mut connection,
        &context.events,
        batch.project_id,
        Reporter::Worker { batch_id: batch.id },
        report,
    )
    .await
    .map_err(|error| match error {
        FindingError::Invalid(reason) => ToolError::Invalid(reason),
        error => internal(scope, OPERATION, &error),
    })?;
    Ok(findings::answer(&reported))
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

/// What recording a pull request reads first: the caller's batch, still running, the project's
/// GitHub token, and the project's setup, which says who merges.
struct Recording {
    batch: WorkBatch,
    token: Option<SecretString>,
    declared: WorkerSetup,
}

async fn recording(context: &ToolContext, scope: &CallScope) -> Result<Recording, ToolError> {
    const OPERATION: &str = "worker.pull_request";

    let worker = scope.worker()?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let batch = work_batch::find(&mut connection, worker.batch_id)
        .await
        .map_err(|error| internal(scope, OPERATION, &error))?;
    if batch.state != BatchState::Running || batch.project_id != worker.project_id {
        return Err(ToolError::Invalid(format!(
            "batch {} is no longer running, so it records nothing; end your turn",
            worker.batch_id
        )));
    }
    let project = project::find(&mut connection, batch.project_id)
        .await
        .map_err(|error| internal(scope, OPERATION, &error))?;
    let workspace_default = github_credential::find_default(&mut connection)
        .await
        .map_err(|error| internal(scope, OPERATION, &error))?
        .map(|credential| credential.id);
    let token = github_token::open_for_session(
        &mut connection,
        &context.cipher,
        project.github_credential(workspace_default),
    )
    .await
    .map_err(|error| internal(scope, OPERATION, &error))?;
    let declared = worker_setup::find(&mut connection, batch.project_id)
        .await
        .map_err(|error| internal(scope, OPERATION, &error))?;
    Ok(Recording {
        batch,
        token,
        declared,
    })
}

/// `found`'s number, when it is `batch`'s worker's own: opened by the project's account during
/// the batch, from the repository itself. Only the worker's own work is recorded, so only the
/// worker's own work is ever merged.
fn own_number(
    found: &PullRequest,
    batch: &WorkBatch,
    reference: &IssueRef,
) -> Result<i32, ToolError> {
    let opened_by_account = found
        .author
        .as_deref()
        .is_some_and(|author| author.eq_ignore_ascii_case(&found.viewer));
    if !opened_by_account {
        return Err(ToolError::Invalid(format!(
            "{reference} was opened by {}, not by the project's account {}; record only the pull \
             requests you opened",
            found
                .author
                .as_deref()
                .unwrap_or("an account GitHub no longer has"),
            found.viewer
        )));
    }
    if found.is_cross_repository {
        return Err(ToolError::Invalid(format!(
            "{reference} is from a fork; open it from a branch of {} itself",
            found.repository
        )));
    }
    if found.created_at < batch.started_at.unwrap_or(batch.created_at) {
        return Err(ToolError::Invalid(format!(
            "{reference} was opened before this batch started, so it is not this batch's work; \
             mention it in your result instead"
        )));
    }
    i32::try_from(found.number).map_err(|_too_large| {
        ToolError::Invalid(format!("{reference} has a number Elysium cannot hold"))
    })
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
