// Copyright © 2026 Jalapeno Labs

//! What a running batch's worker records: the Claude Code session its turns resume, its log,
//! how it said its last turn ended, and what it waits on. See `docs/engine.md`, Workers.
//!
//! A worker says how a turn ended through its own tools (`crate::tools::worker`), answered by the
//! API, which holds no claim on the batch's slot. So a tool only files a [`Report`] on the row
//! ([`report`]); the engine whose worker ran the turn takes it once the turn is over
//! ([`take_report`]) and acts on it under the slot's token: [`super::finish`] for a batch done,
//! [`wait`] for one that waits on a review, CI, or an answer while its worker keeps its container.
//! A question its worker waits on wakes its Project Manager, whose answer ([`answer`]) ends the
//! wait at once.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{BatchState, EventKind, Slot, WorkBatch, record};
use crate::database::schema::{work_batch_events, work_batches};
use crate::models::project_manager;
use crate::models::user::CODING_AGENT_USER_ID;
use crate::runner::configuration::Checkout;

/// The longest a wait's detail, matching `work_batches.waiting_detail`.
pub const WAIT_DETAIL_MAX_CHARACTERS: usize = 4_000;

/// What a running batch's worker waits on, keeping its container meanwhile.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::WorkBatchWait"]
#[serde(rename_all = "snake_case")]
pub enum Wait {
    /// A review of its pull request.
    Review,
    /// A CI run on its work.
    Ci,
    /// An answer to a question only a person, or its Project Manager, can give.
    Question,
}

/// How a worker said its turn ended, filed by its tools for the engine to act on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Report {
    /// The batch is done, or cannot be done: `result` says what it did.
    Finished { succeeded: bool, result: String },
    /// It waits on `on`, saying what for, and looks again at `until` unless answered first.
    #[serde(rename_all = "camelCase")]
    Waiting {
        on: Wait,
        detail: String,
        until: DateTime<Utc>,
    },
}

/// Files `report` for running batch `batch_id` of project `project_id`, replacing any filed
/// earlier in the turn: the last word of a turn is the one acted on. `false` when the batch is
/// not running in that project.
///
/// # Errors
/// Propagates any database error.
pub async fn report(
    connection: &mut AsyncPgConnection,
    batch_id: Uuid,
    project_id: Uuid,
    report: &Report,
) -> QueryResult<bool> {
    let filed = diesel::update(
        work_batches::table
            .find(batch_id)
            .filter(work_batches::project_id.eq(project_id))
            .filter(work_batches::state.eq(BatchState::Running)),
    )
    .set(work_batches::worker_report.eq(json!(report)))
    .execute(connection)
    .await?;
    Ok(filed == 1)
}

/// Takes the report `slot`'s worker filed in its last turn, clearing it. `None` when it filed
/// none, or the slot was lost. A stored report this build cannot read is logged and dropped,
/// as one never filed: the worker is asked again.
///
/// # Errors
/// Propagates any database error.
pub async fn take_report(
    connection: &mut AsyncPgConnection,
    slot: &Slot,
) -> QueryResult<Option<Report>> {
    connection
        .transaction(async |connection| {
            let filed: Option<Option<Value>> = work_batches::table
                .find(slot.batch.id)
                .filter(work_batches::claim_token.eq(slot.token))
                .select(work_batches::worker_report)
                .for_update()
                .first(connection)
                .await
                .optional()?;
            let Some(Some(filed)) = filed else {
                return Ok(None);
            };
            diesel::update(work_batches::table.find(slot.batch.id))
                .set(work_batches::worker_report.eq(None::<Value>))
                .execute(connection)
                .await?;
            match serde_json::from_value(filed) {
                Ok(report) => Ok(Some(report)),
                Err(error) => {
                    tracing::event!(
                        name: "work_batch.report.unreadable",
                        tracing::Level::WARN,
                        work_batch.id = %slot.batch.id,
                        error.message = %error,
                        "a worker's filed report does not read; asking the worker again",
                    );
                    Ok(None)
                }
            }
        })
        .await
}

/// Records that `slot`'s worker waits on `on`, saying `detail`, until `until`, as the coding
/// agent, keeping its container. A question wakes the batch's Project Manager in the same
/// transaction, so it can answer. `None` when the slot was lost or the batch is not running.
///
/// # Errors
/// Propagates any database error, including a detail outside 1 to 4,000 characters.
pub async fn wait(
    connection: &mut AsyncPgConnection,
    slot: &Slot,
    on: Wait,
    detail: &str,
    until: DateTime<Utc>,
) -> QueryResult<Option<WorkBatch>> {
    connection
        .transaction(async |connection| {
            let Some(waiting) = diesel::update(
                work_batches::table
                    .find(slot.batch.id)
                    .filter(work_batches::claim_token.eq(slot.token))
                    .filter(work_batches::state.eq(BatchState::Running)),
            )
            .set((
                work_batches::waiting_on.eq(on),
                work_batches::waiting_detail.eq(detail),
                work_batches::waiting_until.eq(until),
            ))
            .returning(WorkBatch::as_returning())
            .get_result(connection)
            .await
            .optional()?
            else {
                return Ok(None);
            };
            record(
                connection,
                waiting.id,
                EventKind::Waiting,
                json!({ "on": on, "detail": detail, "until": until }),
                CODING_AGENT_USER_ID,
            )
            .await?;
            if on == Wait::Question {
                project_manager::wake_for_batch(
                    connection,
                    waiting.project_id,
                    waiting.id,
                    CODING_AGENT_USER_ID,
                )
                .await?;
            }
            Ok(Some(waiting))
        })
        .await
}

/// Ends the wait of `slot`'s worker, which starts its next turn. `None` when the slot was lost.
///
/// # Errors
/// Propagates any database error.
pub async fn stop_waiting(
    connection: &mut AsyncPgConnection,
    slot: &Slot,
) -> QueryResult<Option<WorkBatch>> {
    diesel::update(
        work_batches::table
            .find(slot.batch.id)
            .filter(work_batches::claim_token.eq(slot.token)),
    )
    .set((
        work_batches::waiting_on.eq(None::<Wait>),
        work_batches::waiting_detail.eq(None::<String>),
        work_batches::waiting_until.eq(None::<DateTime<Utc>>),
    ))
    .returning(WorkBatch::as_returning())
    .get_result(connection)
    .await
    .optional()
}

/// Why an answer was refused.
#[derive(Debug, thiserror::Error)]
pub enum AnswerError {
    /// No batch of the project's waits on a question by that id; the message says what it is.
    #[error("{0}")]
    Refused(String),
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// Answers the question project `project_id`'s running batch `batch_id` waits on, as `by`, and
/// ends its wait now, so its worker's next turn starts with the answer. The caller then sends
/// `Signal::WorkerPool`, which the waiting worker listens for.
///
/// # Errors
/// [`AnswerError::Refused`] when the project has no such batch waiting on a question; any
/// database error.
pub async fn answer(
    connection: &mut AsyncPgConnection,
    batch_id: Uuid,
    project_id: Uuid,
    answer: &str,
    by: Uuid,
) -> Result<WorkBatch, AnswerError> {
    connection
        .transaction(async |connection| {
            let batch: Option<WorkBatch> = work_batches::table
                .find(batch_id)
                .filter(work_batches::project_id.eq(project_id))
                .select(WorkBatch::as_select())
                .for_update()
                .first(connection)
                .await
                .optional()?;
            let Some(batch) = batch else {
                return Err(AnswerError::Refused(format!(
                    "batch {batch_id} is not one of this project's; call manager_batches for them"
                )));
            };
            if batch.state != BatchState::Running || batch.waiting_on != Some(Wait::Question) {
                return Err(AnswerError::Refused(format!(
                    "batch {batch_id} waits on no question; only a running batch whose worker \
                     asked one takes an answer"
                )));
            }
            let answered = diesel::update(work_batches::table.find(batch_id))
                .set(work_batches::waiting_until.eq(diesel::dsl::now))
                .returning(WorkBatch::as_returning())
                .get_result(connection)
                .await?;
            record(
                connection,
                batch_id,
                EventKind::Answered,
                json!({ "answer": answer }),
                by,
            )
            .await?;
            Ok(answered)
        })
        .await
}

/// The answers given to batch `batch_id` since its worker last began to wait, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn answers_since_waiting(
    connection: &mut AsyncPgConnection,
    batch_id: Uuid,
) -> QueryResult<Vec<String>> {
    let last_wait: Option<Uuid> = work_batch_events::table
        .filter(work_batch_events::work_batch_id.eq(batch_id))
        .filter(work_batch_events::kind.eq(EventKind::Waiting.as_str()))
        .select(work_batch_events::id)
        .order(work_batch_events::id.desc())
        .first(connection)
        .await
        .optional()?;
    let Some(last_wait) = last_wait else {
        return Ok(Vec::new());
    };
    let answers: Vec<Value> = work_batch_events::table
        .filter(work_batch_events::work_batch_id.eq(batch_id))
        .filter(work_batch_events::kind.eq(EventKind::Answered.as_str()))
        .filter(work_batch_events::id.gt(last_wait))
        .select(work_batch_events::data)
        .order(work_batch_events::id)
        .load(connection)
        .await?;
    Ok(answers
        .iter()
        .filter_map(|data| data.get("answer").and_then(Value::as_str))
        .map(str::to_owned)
        .collect())
}

/// Keeps the Claude Code session `slot`'s worker's last turn reported, for the next to resume.
///
/// # Errors
/// Propagates any database error.
pub async fn set_session(
    connection: &mut AsyncPgConnection,
    slot: &Slot,
    session_id: &str,
) -> QueryResult<()> {
    diesel::update(
        work_batches::table
            .find(slot.batch.id)
            .filter(work_batches::claim_token.eq(slot.token)),
    )
    .set(work_batches::worker_session_id.eq(session_id))
    .execute(connection)
    .await
    .map(drop)
}

/// Records `checkout` as the configuration `slot`'s worker runs on, which the batch's page shows.
///
/// # Errors
/// Propagates any database error.
pub async fn set_configuration(
    connection: &mut AsyncPgConnection,
    slot: &Slot,
    checkout: &Checkout,
) -> QueryResult<()> {
    let stored = serde_json::to_value(checkout).expect("a checkout serializes");
    diesel::update(
        work_batches::table
            .find(slot.batch.id)
            .filter(work_batches::claim_token.eq(slot.token)),
    )
    .set(work_batches::configuration.eq(stored))
    .execute(connection)
    .await
    .map(drop)
}

/// Stores `log` as `slot`'s worker's log, already scrubbed, and within the 64 KiB
/// `work_batches.worker_log` allows, as a Project Manager's turn log keeps
/// (`crate::project_managers::runner::LOG_MAX_BYTES`).
///
/// # Errors
/// Propagates any database error.
pub async fn set_log(
    connection: &mut AsyncPgConnection,
    slot: &Slot,
    log: &str,
) -> QueryResult<()> {
    diesel::update(
        work_batches::table
            .find(slot.batch.id)
            .filter(work_batches::claim_token.eq(slot.token)),
    )
    .set(work_batches::worker_log.eq(log))
    .execute(connection)
    .await
    .map(drop)
}

/// Batch `batch_id`'s worker's log, empty before its worker said anything.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] for an unknown batch; any other database error.
pub async fn log(connection: &mut AsyncPgConnection, batch_id: Uuid) -> QueryResult<String> {
    work_batches::table
        .find(batch_id)
        .select(work_batches::worker_log)
        .first(connection)
        .await
}

/// The states of the batches among `ids`, for removing what a worker left behind for a batch no
/// worker runs any more.
///
/// # Errors
/// Propagates any database error.
pub async fn states(
    connection: &mut AsyncPgConnection,
    ids: &[Uuid],
) -> QueryResult<Vec<(Uuid, BatchState)>> {
    work_batches::table
        .filter(work_batches::id.eq_any(ids))
        .select((work_batches::id, work_batches::state))
        .load(connection)
        .await
}

#[cfg(test)]
mod tests;
