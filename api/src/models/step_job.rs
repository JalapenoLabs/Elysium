// Copyright © 2026 Jalapeno Labs

//! Step jobs: each step of a workflow run as a row the engine claims, runs, and records.
//!
//! # The queue
//!
//! [`claim`] takes queued steps that are due, and running steps whose lease lapsed, with
//! `FOR UPDATE SKIP LOCKED`, so any number of claimers never take one step twice. Each claim
//! sets a new `claim_token` and a [`LEASE`], and counts an attempt. While the step runs its
//! engine [renews](renew) the lease; an engine that died stops renewing, and its step is claimed
//! again once the lease lapses. Every write of an attempt ([`renew`], [`append_log`],
//! [`finish`], [`release`]) is fenced by its token, so an engine whose lease ran out can never
//! record over the attempt that replaced it, and answers whether it still held the claim.
//!
//! A step may therefore run more than once for one success: when its engine dies, or loses its
//! lease mid-step. Steps are written to be idempotent for that reason (`docs/engine.md`, Runs).
//!
//! # Attempts
//!
//! A step that fails retryably is queued again after a backoff that doubles, until it has had
//! [`MAX_ATTEMPTS`]; then, or when it fails for good, it is failed, and its run stops with the
//! reason. A step whose engine kept dying is failed the same way by [`reap`] once its last
//! attempt's lease lapses. Releasing a step at shutdown does not count the attempt.
//!
//! # A person's actions
//!
//! A person may [`retry`] a failed step, [`skip`] one that has not run, failed, or is held, and
//! [`resume`] a held one. Each is recorded in the run's history with who did it.

use chrono::{DateTime, TimeDelta, Utc};
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::database::schema::step_jobs;
use crate::models::workflow_run::{self, Actor, HistoryKind, Settled};

diesel::define_sql_function! {
    /// Postgres's `right`: the last `count` characters of `text`, which keeps a log's newest.
    fn right(text: diesel::sql_types::Text, count: diesel::sql_types::Integer) -> diesel::sql_types::Text;
}

/// How many times a step is claimed before a failure, or an engine that keeps dying under it,
/// fails it for good. Three rides out a provider's brief outage or one engine restart without
/// leaving a step that can never succeed retrying all day.
pub const MAX_ATTEMPTS: i32 = 3;

/// How long a claim lasts unless renewed. Its engine renews it four times as often, so a slow
/// database never lets a running step's lease lapse; short enough that a step whose engine died
/// is taken again within a minute.
pub const LEASE: TimeDelta = TimeDelta::seconds(60);

/// The wait before a failed step's second attempt; each attempt after doubles it, up to
/// [`RETRY_DELAY_CAP`]. Long enough for a rate limit or a restarting service to clear.
const RETRY_DELAY: TimeDelta = TimeDelta::seconds(30);

/// The longest wait between attempts.
const RETRY_DELAY_CAP: TimeDelta = TimeDelta::minutes(10);

/// The newest characters of a step's log that are kept, as the column's check holds them.
const LOG_LIMIT: i32 = 65_536;

/// The longest summary and error a step's row holds, as the column checks hold them.
const SUMMARY_LIMIT: usize = 500;
const ERROR_LIMIT: usize = 2000;

/// The reason a step is failed with when its engine stopped renewing every attempt it had.
const ABANDONED: &str =
    "The engine running this step stopped before it finished, on every attempt.";

/// Where a step stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize)]
#[ExistingTypePath = "crate::database::schema::sql_types::StepJobState"]
#[serde(rename_all = "snake_case")]
pub enum StepJobState {
    /// The run has not reached it.
    Pending,
    /// Waiting to be claimed, from `available_at`.
    Queued,
    /// An engine holds it under a lease.
    Running,
    /// It finished and asks a person to look before the run goes on.
    Held,
    /// Its last attempt failed, and it is not tried again on its own.
    Failed,
    /// Someone skipped it.
    Skipped,
    Succeeded,
}

impl StepJobState {
    /// The name clients receive, as the state's JSON says it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Held => "held",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Succeeded => "succeeded",
        }
    }
}

/// A stored step, without its log ([`log_of`]) or the claim an engine holds it under.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = step_jobs, check_for_backend(diesel::pg::Pg))]
pub struct StepJob {
    pub id: Uuid,
    pub run_id: Uuid,
    /// Orders the run's steps; never shown, since steps are named.
    pub position: i32,
    /// The step's name in code.
    pub step: String,
    pub state: StepJobState,
    /// Set once the run reached it.
    pub input: Option<Value>,
    /// Set exactly when it succeeded or is held.
    pub output: Option<Value>,
    pub summary: Option<String>,
    /// Why its last attempt failed.
    pub error: Option<String>,
    /// Claims since it was last queued by its run or a person.
    pub attempts: i32,
    /// While queued, the earliest it may be claimed.
    pub available_at: Option<DateTime<Utc>>,
    /// When its last attempt started, and ended.
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// A step an engine claimed, and the token fencing every write of this attempt.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = step_jobs, check_for_backend(diesel::pg::Pg))]
pub struct Claim {
    #[diesel(embed)]
    pub job: StepJob,
    #[diesel(select_expression = step_jobs::claim_token.assume_not_null())]
    pub token: Uuid,
}

/// How one attempt at a step ended, its output as the queue stores it. Steps answer in their
/// own types (`crate::workflows::Done`), which their registration turns into this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// It did its work. `output` is what the next step receives; `summary` is the one line its
    /// row collapses to.
    Succeeded {
        output: Value,
        summary: Option<String>,
    },
    /// It did its work and asks a person to look before the run goes on, such as approving a
    /// corrected transcript. `summary` says what to look at; resuming passes `output` on.
    Held { output: Value, summary: String },
    /// It could not do its work. A `retryable` failure, such as a provider that did not answer,
    /// is tried again after a backoff while attempts remain; any other fails the step at once.
    Failed { reason: String, retryable: bool },
}

/// Why a person's action on a step was refused.
#[derive(Debug, thiserror::Error)]
pub enum StepActionError {
    /// Includes [`DieselError::NotFound`] for an unknown run, or a step not in it.
    #[error(transparent)]
    Database(#[from] DieselError),
    /// The step's state does not allow it; the message says what does.
    #[error("{0}")]
    Refused(&'static str),
}

/// What [`claim`] took: the claims, and the runs they set going, settled.
#[derive(Debug)]
pub struct Claimed {
    pub claims: Vec<Claim>,
    pub settled: Vec<Settled>,
}

/// Claims up to `limit` steps that are due at `at`, oldest first: queued steps whose backoff has
/// passed, and running steps whose lease lapsed with attempts left. Each claimed step is running
/// under a new token and lease, its attempt counted. Then settles each claimed step's run, which
/// is running from its first claim.
///
/// The claim locks only the steps it takes, and skips any another transaction holds, so it never
/// waits on another claimer or on a person's action. It commits before any run is settled, so
/// it never holds a step while waiting for a run's lock either.
///
/// # Errors
/// Propagates any database error.
pub async fn claim(
    connection: &mut AsyncPgConnection,
    at: DateTime<Utc>,
    limit: i64,
) -> QueryResult<Claimed> {
    let claims = take_due(connection, at, limit).await?;
    let mut by_run: Vec<(Uuid, Vec<Uuid>)> = Vec::new();
    for claim in &claims {
        match by_run
            .iter_mut()
            .find(|(run_id, _)| *run_id == claim.job.run_id)
        {
            Some((_, moved)) => moved.push(claim.job.id),
            None => by_run.push((claim.job.run_id, vec![claim.job.id])),
        }
    }
    let mut settled = Vec::with_capacity(by_run.len());
    for (run_id, moved) in by_run {
        let run_settled = connection
            .transaction(async move |connection| {
                let run = workflow_run::lock(connection, run_id).await?;
                workflow_run::settle(connection, run, moved, at).await
            })
            .await?;
        settled.push(run_settled);
    }
    Ok(Claimed { claims, settled })
}

/// The claim itself, in one transaction: locks the due steps, skipping any held, and takes them.
async fn take_due(
    connection: &mut AsyncPgConnection,
    at: DateTime<Utc>,
    limit: i64,
) -> QueryResult<Vec<Claim>> {
    connection
        .transaction(async move |connection| {
            let due: Vec<Uuid> = step_jobs::table
                .filter(
                    step_jobs::state
                        .eq(StepJobState::Queued)
                        .and(step_jobs::available_at.le(at))
                        .or(step_jobs::state
                            .eq(StepJobState::Running)
                            .and(step_jobs::lease_expires_at.lt(at))
                            .and(step_jobs::attempts.lt(MAX_ATTEMPTS))),
                )
                .order(step_jobs::id)
                .limit(limit)
                .select(step_jobs::id)
                .for_update()
                .skip_locked()
                .load(connection)
                .await?;
            if due.is_empty() {
                return Ok(Vec::new());
            }
            diesel::update(step_jobs::table.filter(step_jobs::id.eq_any(&due)))
                .set((
                    step_jobs::state.eq(StepJobState::Running),
                    step_jobs::attempts.eq(step_jobs::attempts + 1),
                    // Each row its own, so one claim's token fences one step.
                    step_jobs::claim_token.eq(diesel::dsl::sql::<
                        diesel::sql_types::Nullable<diesel::sql_types::Uuid>,
                    >("gen_random_uuid()")),
                    step_jobs::lease_expires_at.eq(at + LEASE),
                    step_jobs::available_at.eq(None::<DateTime<Utc>>),
                    step_jobs::started_at.eq(at),
                    step_jobs::finished_at.eq(None::<DateTime<Utc>>),
                ))
                .returning(Claim::as_returning())
                .get_results(connection)
                .await
        })
        .await
}

/// Extends `claim`'s lease from `at`. False when the claim was lost: its lease lapsed and
/// another engine took the step, so this attempt must stop.
///
/// # Errors
/// Propagates any database error.
pub async fn renew(
    connection: &mut AsyncPgConnection,
    claim: &Claim,
    at: DateTime<Utc>,
) -> QueryResult<bool> {
    let renewed = diesel::update(
        step_jobs::table
            .find(claim.job.id)
            .filter(step_jobs::claim_token.eq(claim.token))
            .filter(step_jobs::state.eq(StepJobState::Running)),
    )
    .set(step_jobs::lease_expires_at.eq(at + LEASE))
    .execute(connection)
    .await?;
    Ok(renewed == 1)
}

/// Appends `text` to `claim`'s log, keeping the newest [`LOG_LIMIT`] characters. False when the
/// claim was lost.
///
/// # Errors
/// Propagates any database error.
pub async fn append_log(
    connection: &mut AsyncPgConnection,
    claim: &Claim,
    text: &str,
) -> QueryResult<bool> {
    let appended = diesel::update(
        step_jobs::table
            .find(claim.job.id)
            .filter(step_jobs::claim_token.eq(claim.token))
            .filter(step_jobs::state.eq(StepJobState::Running)),
    )
    .set(step_jobs::log.eq(right(step_jobs::log.concat(text), LOG_LIMIT)))
    .execute(connection)
    .await?;
    Ok(appended == 1)
}

/// Records how `claim`'s attempt ended, at `at`, and settles its run: a success or a hold
/// moves the run on, a retryable failure with attempts left queues the step after its backoff,
/// and any other failure fails the step and stops the run. `None` when the claim was lost, and
/// the outcome is dropped: the attempt that replaced it records its own.
///
/// # Errors
/// Propagates any database error.
pub async fn finish(
    connection: &mut AsyncPgConnection,
    claim: &Claim,
    outcome: StepOutcome,
    at: DateTime<Utc>,
) -> QueryResult<Option<Settled>> {
    connection
        .transaction(async move |connection| {
            let run = workflow_run::lock(connection, claim.job.run_id).await?;
            let still_held: Option<i32> = step_jobs::table
                .find(claim.job.id)
                .filter(step_jobs::claim_token.eq(claim.token))
                .filter(step_jobs::state.eq(StepJobState::Running))
                .for_update()
                .select(step_jobs::attempts)
                .first(connection)
                .await
                .optional()?;
            let Some(attempts) = still_held else {
                return Ok(None);
            };
            let ended = diesel::update(step_jobs::table.find(claim.job.id));
            let unclaimed = (
                step_jobs::claim_token.eq(None::<Uuid>),
                step_jobs::lease_expires_at.eq(None::<DateTime<Utc>>),
                step_jobs::finished_at.eq(at),
            );
            match outcome {
                StepOutcome::Succeeded { output, summary } => {
                    ended
                        .set((
                            unclaimed,
                            step_jobs::state.eq(StepJobState::Succeeded),
                            step_jobs::output.eq(output),
                            step_jobs::summary
                                .eq(summary.and_then(|text| bounded(&text, SUMMARY_LIMIT))),
                            step_jobs::error.eq(None::<String>),
                        ))
                        .execute(connection)
                        .await?;
                }
                StepOutcome::Held { output, summary } => {
                    ended
                        .set((
                            unclaimed,
                            step_jobs::state.eq(StepJobState::Held),
                            step_jobs::output.eq(output),
                            step_jobs::summary.eq(bounded(&summary, SUMMARY_LIMIT)),
                            step_jobs::error.eq(None::<String>),
                        ))
                        .execute(connection)
                        .await?;
                }
                StepOutcome::Failed { reason, retryable } => {
                    let reason = bounded(&reason, ERROR_LIMIT)
                        .unwrap_or_else(|| "The step failed without saying why.".to_owned());
                    let line = format!("Attempt {attempts} failed: {reason}\n");
                    if retryable && attempts < MAX_ATTEMPTS {
                        ended
                            .set((
                                unclaimed,
                                step_jobs::state.eq(StepJobState::Queued),
                                step_jobs::available_at.eq(at + retry_delay(attempts)),
                                step_jobs::error.eq(reason),
                                step_jobs::log.eq(right(step_jobs::log.concat(&line), LOG_LIMIT)),
                            ))
                            .execute(connection)
                            .await?;
                    } else {
                        ended
                            .set((
                                unclaimed,
                                step_jobs::state.eq(StepJobState::Failed),
                                step_jobs::error.eq(reason),
                                step_jobs::log.eq(right(step_jobs::log.concat(&line), LOG_LIMIT)),
                            ))
                            .execute(connection)
                            .await?;
                    }
                }
            }
            workflow_run::settle(connection, run, vec![claim.job.id], at)
                .await
                .map(Some)
        })
        .await
}

/// Gives `claim`'s step back unfinished, at `at`, for the next claim to take at once, without
/// counting the attempt: its engine is shutting down. False when the claim was already lost.
///
/// # Errors
/// Propagates any database error.
pub async fn release(
    connection: &mut AsyncPgConnection,
    claim: &Claim,
    at: DateTime<Utc>,
) -> QueryResult<bool> {
    let released = diesel::update(
        step_jobs::table
            .find(claim.job.id)
            .filter(step_jobs::claim_token.eq(claim.token))
            .filter(step_jobs::state.eq(StepJobState::Running)),
    )
    .set((
        step_jobs::state.eq(StepJobState::Queued),
        step_jobs::attempts.eq(step_jobs::attempts - 1),
        step_jobs::claim_token.eq(None::<Uuid>),
        step_jobs::lease_expires_at.eq(None::<DateTime<Utc>>),
        step_jobs::available_at.eq(at),
    ))
    .execute(connection)
    .await?;
    Ok(released == 1)
}

/// Fails every step whose engine stopped renewing its last attempt by `at`, and settles their
/// runs, which stop with the reason. Steps with attempts left are claimed again instead.
///
/// # Errors
/// Propagates any database error.
pub async fn reap(
    connection: &mut AsyncPgConnection,
    at: DateTime<Utc>,
) -> QueryResult<Vec<Settled>> {
    let abandoned: Vec<(Uuid, Uuid)> = step_jobs::table
        .filter(step_jobs::state.eq(StepJobState::Running))
        .filter(step_jobs::lease_expires_at.lt(at))
        .filter(step_jobs::attempts.ge(MAX_ATTEMPTS))
        .select((step_jobs::id, step_jobs::run_id))
        .load(connection)
        .await?;
    let mut settled = Vec::with_capacity(abandoned.len());
    for (step_id, run_id) in abandoned {
        let reaped = connection
            .transaction(async move |connection| {
                let run = workflow_run::lock(connection, run_id).await?;
                // Taken again by the run's lock order, and checked again under it.
                let lapsed: Option<Uuid> = step_jobs::table
                    .find(step_id)
                    .filter(step_jobs::state.eq(StepJobState::Running))
                    .filter(step_jobs::lease_expires_at.lt(at))
                    .for_update()
                    .select(step_jobs::id)
                    .first(connection)
                    .await
                    .optional()?;
                if lapsed.is_none() {
                    return Ok(None);
                }
                diesel::update(step_jobs::table.find(step_id))
                    .set((
                        step_jobs::state.eq(StepJobState::Failed),
                        step_jobs::claim_token.eq(None::<Uuid>),
                        step_jobs::lease_expires_at.eq(None::<DateTime<Utc>>),
                        step_jobs::finished_at.eq(at),
                        step_jobs::error.eq(ABANDONED),
                        step_jobs::log.eq(right(
                            step_jobs::log.concat(format!("{ABANDONED}\n")),
                            LOG_LIMIT,
                        )),
                    ))
                    .execute(connection)
                    .await?;
                workflow_run::settle(connection, run, vec![step_id], at)
                    .await
                    .map(Some)
            })
            .await?;
        settled.extend(reaped);
    }
    Ok(settled)
}

/// When the next queued step becomes due, if any is queued.
///
/// # Errors
/// Propagates any database error.
pub async fn next_due(connection: &mut AsyncPgConnection) -> QueryResult<Option<DateTime<Utc>>> {
    step_jobs::table
        .filter(step_jobs::state.eq(StepJobState::Queued))
        .select(diesel::dsl::min(step_jobs::available_at))
        .first(connection)
        .await
}

/// Step `step_id`'s log, every attempt's, when it is a step of run `run_id`.
///
/// # Errors
/// `NotFound` for an unknown step, or one of another run.
pub async fn log_of(
    connection: &mut AsyncPgConnection,
    run_id: Uuid,
    step_id: Uuid,
) -> QueryResult<String> {
    step_jobs::table
        .find(step_id)
        .filter(step_jobs::run_id.eq(run_id))
        .select(step_jobs::log)
        .first(connection)
        .await
}

/// Queues failed step `step_id` of run `run_id` again, with a fresh set of attempts, and
/// starts its run going again.
///
/// # Errors
/// `Refused` unless the step failed; `NotFound` for an unknown run or step.
pub async fn retry(
    connection: &mut AsyncPgConnection,
    run_id: Uuid,
    step_id: Uuid,
    actor: Actor,
) -> Result<Settled, StepActionError> {
    connection
        .transaction(async move |connection| {
            let (run, step) = locked(connection, run_id, step_id).await?;
            if step.state != StepJobState::Failed {
                return Err(StepActionError::Refused(
                    "Only a failed step can be retried.",
                ));
            }
            diesel::update(step_jobs::table.find(step.id))
                .set((
                    step_jobs::state.eq(StepJobState::Queued),
                    step_jobs::attempts.eq(0),
                    step_jobs::error.eq(None::<String>),
                    step_jobs::available_at.eq(actor.at),
                ))
                .execute(connection)
                .await?;
            let data = json!({ "error": step.error });
            workflow_run::record(
                connection,
                run.id,
                Some(step.id),
                HistoryKind::Retried,
                actor,
                data,
            )
            .await?;
            Ok(workflow_run::settle(connection, run, vec![step.id], actor.at).await?)
        })
        .await
}

/// Skips step `step_id` of run `run_id`: the step after it receives the input this one would
/// have, and a run this one stopped or held goes on.
///
/// # Errors
/// `Refused` while the step runs or once it succeeded or was skipped; `NotFound` for an
/// unknown run or step.
pub async fn skip(
    connection: &mut AsyncPgConnection,
    run_id: Uuid,
    step_id: Uuid,
    actor: Actor,
) -> Result<Settled, StepActionError> {
    connection
        .transaction(async move |connection| {
            let (run, step) = locked(connection, run_id, step_id).await?;
            if !matches!(
                step.state,
                StepJobState::Pending
                    | StepJobState::Queued
                    | StepJobState::Failed
                    | StepJobState::Held
            ) {
                return Err(StepActionError::Refused(
                    "A step can be skipped before it runs, once it failed, or while it is held.",
                ));
            }
            diesel::update(step_jobs::table.find(step.id))
                .set((
                    step_jobs::state.eq(StepJobState::Skipped),
                    step_jobs::available_at.eq(None::<DateTime<Utc>>),
                    step_jobs::output.eq(None::<Value>),
                    step_jobs::summary.eq(None::<String>),
                    step_jobs::error.eq(None::<String>),
                    step_jobs::finished_at.eq(actor.at),
                ))
                .execute(connection)
                .await?;
            let data = json!({ "from": step.state.as_str() });
            workflow_run::record(
                connection,
                run.id,
                Some(step.id),
                HistoryKind::Skipped,
                actor,
                data,
            )
            .await?;
            Ok(workflow_run::settle(connection, run, vec![step.id], actor.at).await?)
        })
        .await
}

/// Lets held step `step_id` of run `run_id` through: it succeeds with the output it was held
/// with, and the run goes on.
///
/// # Errors
/// `Refused` unless the step is held; `NotFound` for an unknown run or step.
pub async fn resume(
    connection: &mut AsyncPgConnection,
    run_id: Uuid,
    step_id: Uuid,
    actor: Actor,
) -> Result<Settled, StepActionError> {
    connection
        .transaction(async move |connection| {
            let (run, step) = locked(connection, run_id, step_id).await?;
            if step.state != StepJobState::Held {
                return Err(StepActionError::Refused("Only a held step can be resumed."));
            }
            diesel::update(step_jobs::table.find(step.id))
                .set(step_jobs::state.eq(StepJobState::Succeeded))
                .execute(connection)
                .await?;
            workflow_run::record(
                connection,
                run.id,
                Some(step.id),
                HistoryKind::Resumed,
                actor,
                json!({}),
            )
            .await?;
            Ok(workflow_run::settle(connection, run, vec![step.id], actor.at).await?)
        })
        .await
}

/// Run `run_id` and its step `step_id`, both locked, in the run-first order every write takes.
async fn locked(
    connection: &mut AsyncPgConnection,
    run_id: Uuid,
    step_id: Uuid,
) -> QueryResult<(workflow_run::WorkflowRun, StepJob)> {
    let run = workflow_run::lock(connection, run_id).await?;
    let step = step_jobs::table
        .find(step_id)
        .filter(step_jobs::run_id.eq(run_id))
        .for_update()
        .select(StepJob::as_select())
        .first(connection)
        .await?;
    Ok((run, step))
}

/// `text` trimmed and cut to `limit` characters; `None` when nothing is left.
fn bounded(text: &str, limit: usize) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(limit).collect())
}

/// The wait after a failed attempt, the `attempts`th: [`RETRY_DELAY`], doubling each attempt,
/// at most [`RETRY_DELAY_CAP`].
fn retry_delay(attempts: i32) -> TimeDelta {
    let doublings = u32::try_from(attempts.saturating_sub(1))
        .unwrap_or(0)
        .min(16);
    RETRY_DELAY
        .checked_mul(1_i32 << doublings)
        .map_or(RETRY_DELAY_CAP, |delay| delay.min(RETRY_DELAY_CAP))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_wait_longer_each_attempt_up_to_the_cap() {
        assert_eq!(retry_delay(1), TimeDelta::seconds(30));
        assert_eq!(retry_delay(2), TimeDelta::seconds(60));
        assert_eq!(retry_delay(3), TimeDelta::seconds(120));
        assert_eq!(retry_delay(10), RETRY_DELAY_CAP);
        assert_eq!(retry_delay(i32::MAX), RETRY_DELAY_CAP);
    }

    #[test]
    fn summaries_and_reasons_are_trimmed_cut_and_never_empty() {
        assert_eq!(bounded("  done  ", 10), Some("done".to_owned()));
        assert_eq!(bounded("   ", 10), None);
        assert_eq!(
            bounded("ééééé", 3),
            Some("ééé".to_owned()),
            "cut by characters"
        );
    }
}
