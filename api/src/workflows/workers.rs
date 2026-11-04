// Copyright © 2026 Jalapeno Labs

//! The engine's step workers: they claim due steps and run them, [`CONCURRENCY`] at a time.
//!
//! [`Workers::start`] looks at the queue whenever [`Signal::Workflows`] arrives, whenever a step
//! it ran finishes, when the next step waiting out a backoff becomes due, and every
//! [`LOOK_INTERVAL`] in case a signal was lost. Each look:
//!
//! 1. fails every step whose engine stopped renewing its last attempt (`step_job::reap`);
//! 2. claims as many due steps as it has free slots (`step_job::claim`), which also takes back
//!    steps whose engine died with attempts left, once their lease lapses;
//! 3. runs each claimed step in a task of its own.
//!
//! While a step runs, its task renews the lease every [`RENEW_INTERVAL`] and stores what the
//! step logged every [`LOG_FLUSH_INTERVAL`]; a renewal that finds the claim lost stops the step,
//! since another attempt now owns it. A step that ends has its outcome recorded, which settles
//! its run and queues the next step. A running step holds a `WorkflowStep` activity, so
//! auto-update waits for it.
//!
//! At shutdown every running step is stopped where it is and released: its step is queued
//! again without the attempt counting, for the next engine to take at once.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior, interval_at};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{Level, event};

use super::{StepCall, StepLog, StepOutcome, Steps, announce};
use crate::activity::{Activities, ActivityKind};
use crate::database::Pool;
use crate::models::step_job::{self, Claim};
use crate::models::workflow_run;
use crate::realtime::{EventBus, ServerEvent, Signal};

/// How many steps one engine runs at once. A step that runs an agent turn waits for the
/// runner's own slots besides; four keeps a burst of runs from filling the runner while
/// leaving room for cheap steps alongside a slow one.
const CONCURRENCY: usize = 4;

/// How often the queue is looked at without a reason to. Signals are at most once, so this
/// bounds how long a run whose signal was lost waits, and how soon a lapsed lease is noticed.
const LOOK_INTERVAL: Duration = Duration::from_secs(30);

/// How often a running step's lease is renewed: four times per lease, so a renewal or two lost
/// to a slow database never lets it lapse.
const RENEW_INTERVAL: Duration = Duration::from_secs(15);

/// How often what a running step logged is stored, so the Runs page follows it closely without
/// a write and an event per line.
const LOG_FLUSH_INTERVAL: Duration = Duration::from_secs(1);

/// Runs claimed steps. Cheap to clone; every clone is the same workers.
#[derive(Debug, Clone)]
pub struct Workers {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    events: EventBus,
    activity: Activities,
    steps: Steps,
    /// One permit per step that may run at once.
    slots: Arc<Semaphore>,
    /// A step finished, so a slot is free and its run may have queued the next step.
    finished: Notify,
    tracker: TaskTracker,
}

impl std::fmt::Debug for Inner {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Workers")
            .field("steps", &self.steps)
            .field("free_slots", &self.slots.available_permits())
            .finish_non_exhaustive()
    }
}

/// What the workers are built from.
pub struct WorkersDeps {
    pub database: Pool,
    pub events: EventBus,
    /// Where each running step is held, for auto-update to wait on.
    pub activity: Activities,
    /// Every step this build can run.
    pub steps: Steps,
}

impl std::fmt::Debug for WorkersDeps {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkersDeps")
            .field("steps", &self.steps)
            .finish_non_exhaustive()
    }
}

/// How one step's task ended.
enum Ending {
    /// The step returned.
    Returned(StepOutcome),
    /// The engine is stopping.
    Stopping,
    /// A renewal or a log write found the claim gone: another attempt owns the step now.
    Lost,
}

impl Workers {
    pub fn new(deps: WorkersDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                database: deps.database,
                events: deps.events,
                activity: deps.activity,
                steps: deps.steps,
                slots: Arc::new(Semaphore::new(CONCURRENCY)),
                finished: Notify::new(),
                tracker: TaskTracker::new(),
            }),
        }
    }

    /// Runs steps until `shutdown`, then releases every step still running and waits for each
    /// release to be recorded.
    pub fn start(&self, shutdown: CancellationToken) -> JoinHandle<()> {
        let workers = self.clone();
        let mut wake = self
            .inner
            .events
            .wake_on(|signal| *signal == Signal::Workflows);
        tokio::spawn(async move {
            loop {
                let next_look = workers.look(&shutdown).await;
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    () = wake.woken() => {}
                    () = workers.inner.finished.notified() => {}
                    () = tokio::time::sleep(next_look) => {}
                }
            }
            workers.inner.tracker.close();
            workers.inner.tracker.wait().await;
            event!(
                name: "workflows.workers.stopped",
                Level::INFO,
                "the step workers stopped"
            );
        })
    }

    /// One look at the queue: fails what was abandoned, then claims and starts what is due.
    /// Answers how long until the next look is owed.
    async fn look(&self, shutdown: &CancellationToken) -> Duration {
        let looked = async {
            let mut connection = self.inner.database.get().await?;
            let now = Utc::now();
            for settled in step_job::reap(&mut connection, now).await? {
                announce(&self.inner.events, &settled);
            }
            let free = self.inner.slots.available_permits();
            if free > 0 {
                let limit = i64::try_from(free).unwrap_or(i64::MAX);
                let claimed = step_job::claim(&mut connection, now, limit).await?;
                for settled in &claimed.settled {
                    announce(&self.inner.events, settled);
                }
                for claim in claimed.claims {
                    // Never waits: no more were claimed than slots were free, and only this
                    // loop takes slots.
                    let slot = Arc::clone(&self.inner.slots).acquire_owned().await?;
                    let workers = self.clone();
                    let stopping = shutdown.clone();
                    self.inner
                        .tracker
                        .spawn(async move { workers.run(claim, slot, stopping).await });
                }
            }
            let next_due = step_job::next_due(&mut connection).await?;
            anyhow::Ok(next_due)
        };
        match looked.await {
            Ok(Some(due)) => (due - Utc::now())
                .to_std()
                .unwrap_or(Duration::ZERO)
                .min(LOOK_INTERVAL),
            Ok(None) => LOOK_INTERVAL,
            Err(error) => {
                event!(
                    name: "workflows.workers.look_failure",
                    Level::ERROR,
                    error.message = %format!("{error:#}"),
                    "could not look at the step queue; looking again shortly",
                );
                LOOK_INTERVAL
            }
        }
    }

    /// Runs `claim`'s step to its end, renewing its lease and storing its log meanwhile, then
    /// records how it ended.
    async fn run(&self, claim: Claim, slot: OwnedSemaphorePermit, shutdown: CancellationToken) {
        let _activity = self.inner.activity.begin(
            ActivityKind::WorkflowStep,
            format!("run:{}:{}", claim.job.run_id, claim.job.step),
        );
        let log = StepLog::default();
        let ending = match self.inner.steps.find(&claim.job.step) {
            Some(step) => {
                let call = self.call_for(&claim, log.clone()).await;
                match call {
                    Some(call) => self.drive(&claim, step(call), &log, &shutdown).await,
                    // Its run is gone: there is nothing left to record it on.
                    None => Ending::Lost,
                }
            }
            None => Ending::Returned(StepOutcome::Failed {
                reason: format!(
                    "This build of Elysium has no step named {}.",
                    claim.job.step
                ),
                retryable: false,
            }),
        };
        // Whatever it wrote last is stored before how it ended, so the log reads in order.
        self.flush(&claim, &log).await;
        match ending {
            Ending::Returned(outcome) => self.record(&claim, outcome).await,
            Ending::Stopping => self.release(&claim).await,
            Ending::Lost => event!(
                name: "workflows.step.claim_lost",
                Level::WARN,
                workflow.run.id = %claim.job.run_id,
                workflow.step.id = %claim.job.id,
                "a step's claim was lost while it ran; the attempt that took it over records it",
            ),
        }
        drop(slot);
        self.inner.finished.notify_one();
    }

    /// What `claim`'s step is called with: its run's workflow and source with its own input.
    /// `None` when the run cannot be read, so the step cannot be run as it was queued.
    async fn call_for(&self, claim: &Claim, log: StepLog) -> Option<StepCall> {
        let found = async {
            let mut connection = self.inner.database.get().await?;
            anyhow::Ok(workflow_run::find(&mut connection, claim.job.run_id).await?)
        };
        match found.await {
            Ok(run) => Some(StepCall {
                run_id: run.id,
                step_job_id: claim.job.id,
                workflow: run.workflow,
                source_kind: run.source_kind,
                source_id: run.source_id,
                step: claim.job.step.clone(),
                input: claim.job.input.clone().unwrap_or_default(),
                attempt: claim.job.attempts,
                log,
            }),
            Err(error) => {
                event!(
                    name: "workflows.step.run_unreadable",
                    Level::ERROR,
                    workflow.run.id = %claim.job.run_id,
                    error.message = %format!("{error:#}"),
                    "cannot read the run a claimed step belongs to; its lease lapses and it is claimed again",
                );
                None
            }
        }
    }

    /// Polls the step's attempt until it returns, the engine stops, or the claim is lost,
    /// renewing the lease and storing the log meanwhile. The attempt is dropped where it is
    /// unless it returned.
    async fn drive(
        &self,
        claim: &Claim,
        mut attempt: super::StepFuture,
        log: &StepLog,
        shutdown: &CancellationToken,
    ) -> Ending {
        let mut flushes = interval_at(Instant::now() + LOG_FLUSH_INTERVAL, LOG_FLUSH_INTERVAL);
        flushes.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut renewals = interval_at(Instant::now() + RENEW_INTERVAL, RENEW_INTERVAL);
        renewals.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                outcome = &mut attempt => return Ending::Returned(outcome),
                () = shutdown.cancelled() => return Ending::Stopping,
                _ = flushes.tick() => {
                    if !self.flush(claim, log).await {
                        return Ending::Lost;
                    }
                }
                _ = renewals.tick() => {
                    if !self.renew(claim).await {
                        return Ending::Lost;
                    }
                }
            }
        }
    }

    /// Stores what the step logged since the last flush. False when the claim was lost. A
    /// write that fails keeps the text for the next flush.
    async fn flush(&self, claim: &Claim, log: &StepLog) -> bool {
        let text = log.take();
        if text.is_empty() {
            return true;
        }
        let appended = async {
            let mut connection = self.inner.database.get().await?;
            anyhow::Ok(step_job::append_log(&mut connection, claim, &text).await?)
        };
        match appended.await {
            Ok(true) => {
                self.inner.events.publish(&ServerEvent::StepJobChanged {
                    id: claim.job.id,
                    run_id: claim.job.run_id,
                    state: step_job::StepJobState::Running,
                });
                true
            }
            Ok(false) => false,
            Err(error) => {
                event!(
                    name: "workflows.step.log_failure",
                    Level::WARN,
                    workflow.step.id = %claim.job.id,
                    error.message = %format!("{error:#}"),
                    "could not store a step's log; trying again at the next flush",
                );
                log.put_back(text);
                true
            }
        }
    }

    /// Renews `claim`'s lease. False when the claim was lost. A renewal that fails is logged
    /// and tried again at the next interval, since the lease outlasts several of them.
    async fn renew(&self, claim: &Claim) -> bool {
        let renewed = async {
            let mut connection = self.inner.database.get().await?;
            anyhow::Ok(step_job::renew(&mut connection, claim, Utc::now()).await?)
        };
        match renewed.await {
            Ok(held) => held,
            Err(error) => {
                event!(
                    name: "workflows.step.renew_failure",
                    Level::WARN,
                    workflow.step.id = %claim.job.id,
                    error.message = %format!("{error:#}"),
                    "could not renew a step's lease; trying again before it lapses",
                );
                true
            }
        }
    }

    /// Records how `claim`'s attempt ended and announces what it moved. A record that fails
    /// leaves the step running until its lease lapses, when it is claimed and run again.
    async fn record(&self, claim: &Claim, outcome: StepOutcome) {
        let recorded = async {
            let mut connection = self.inner.database.get().await?;
            anyhow::Ok(step_job::finish(&mut connection, claim, outcome, Utc::now()).await?)
        };
        match recorded.await {
            Ok(Some(settled)) => announce(&self.inner.events, &settled),
            Ok(None) => event!(
                name: "workflows.step.claim_lost",
                Level::WARN,
                workflow.run.id = %claim.job.run_id,
                workflow.step.id = %claim.job.id,
                "a step finished after its claim was lost; the attempt that took it over records it",
            ),
            Err(error) => event!(
                name: "workflows.step.record_failure",
                Level::ERROR,
                workflow.step.id = %claim.job.id,
                error.message = %format!("{error:#}"),
                "could not record how a step ended; it runs again once its lease lapses",
            ),
        }
    }

    /// Gives `claim`'s step back for the next engine, without counting the attempt.
    async fn release(&self, claim: &Claim) {
        let released = async {
            let mut connection = self.inner.database.get().await?;
            anyhow::Ok(step_job::release(&mut connection, claim, Utc::now()).await?)
        };
        match released.await {
            Ok(true) => self.inner.events.publish(&ServerEvent::StepJobChanged {
                id: claim.job.id,
                run_id: claim.job.run_id,
                state: step_job::StepJobState::Queued,
            }),
            Ok(false) => {}
            Err(error) => event!(
                name: "workflows.step.release_failure",
                Level::WARN,
                workflow.step.id = %claim.job.id,
                error.message = %format!("{error:#}"),
                "could not give a stopping step back; it is claimed again once its lease lapses",
            ),
        }
    }
}
