// Copyright © 2026 Jalapeno Labs

//! The pool's workers: one per slot, each running its batch in a container of its own until the
//! batch is done. See `docs/engine.md`, Workers.
//!
//! A worker is Claude Code in a container of its own from the workspace image, with its own
//! volume, network, and desktop ([`container`]). The scheduler hands it a slot; the worker starts
//! the batch, readies the container ([`setup`]: the project's tools, the batch's repositories
//! with their submodules, each repository's setup), and runs turns ([`run`]) on a runner of its
//! own, so no worker waits behind the shared workspace's turns. Every turn's prompt is `/goal`,
//! holding out until the worker reports through its tools ([`brief`]): finished, which ends the
//! batch and removes the container, or waiting on a review, CI, or an answer, which keeps the
//! container and starts the next turn when it is time to look again.
//!
//! [`Workers`] is what the scheduler holds: on each look it starts a worker for every slot it
//! holds that has none, stops the worker of a slot it lost, keeps each worker's CPU limit at the
//! pool's share, joins the API to each worker's network again, and removes what a worker left in
//! Docker for a batch no worker runs any more.
//!
//! A worker outlives its engine: an engine that stops interrupts the turn under way and leaves the
//! container; the engine that takes the slot over finds the container, and the next turn resumes
//! the worker's Claude Code session where it stopped.

pub mod brief;
pub mod container;
mod run;
pub mod setup;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{Level, event};
use uuid::Uuid;

use self::container::Containers;
use crate::activity::Activities;
use crate::claude_accounts::TurnSignal;
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::work_batch::BatchState;
use crate::models::work_batch::worker as batch_worker;
use crate::passwords::Passwords;
use crate::realtime::EventBus;
use crate::tools::grants::Grants;

/// How long shutdown waits for workers to record how their turns ended.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(20);

/// What workers are built from.
#[derive(Clone)]
pub struct WorkerDeps {
    pub database: Pool,
    pub cipher: Arc<Cipher>,
    pub events: EventBus,
    pub containers: Containers,
    pub grants: Grants,
    /// The password vault: its tools are granted while it is ready, and every turn's output is
    /// scrubbed of what it handed out.
    pub passwords: Passwords,
    /// Where each running turn says the stack is busy.
    pub activity: Activities,
    /// Tells the Claude account keeper a turn finished.
    pub turn_signal: TurnSignal,
}

impl std::fmt::Debug for WorkerDeps {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkerDeps")
            .field("containers", &self.containers)
            .finish_non_exhaustive()
    }
}

/// The pool's workers. Cheap to clone; every clone is the same set.
#[derive(Debug, Clone)]
pub struct Workers {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    deps: WorkerDeps,
    /// The worker of each slot this engine holds, by batch.
    running: Mutex<HashMap<Uuid, Running>>,
    tracker: TaskTracker,
    shutdown: CancellationToken,
}

/// One worker's handle.
#[derive(Debug)]
struct Running {
    /// The token fencing the slot the worker was started under.
    token: Uuid,
    /// Stops the worker, leaving its container: the slot was lost, or the engine stops.
    stop: CancellationToken,
    /// Set while its container is up, so the look keeps its network and CPU limit.
    ready: Arc<AtomicBool>,
    /// The CPU limit its container was last given.
    nano_cpus: i64,
}

/// One worker, as its task holds it.
#[derive(Debug, Clone)]
struct Worker {
    deps: WorkerDeps,
    batch_id: Uuid,
    token: Uuid,
    stop: CancellationToken,
    ready: Arc<AtomicBool>,
    nano_cpus: i64,
}

impl Workers {
    /// Workers built from `deps`, every one stopped by `shutdown`.
    pub fn new(deps: WorkerDeps, shutdown: &CancellationToken) -> Self {
        Self {
            inner: Arc::new(Inner {
                deps,
                running: Mutex::new(HashMap::new()),
                tracker: TaskTracker::new(),
                shutdown: shutdown.child_token(),
            }),
        }
    }

    /// One look, given the slots the scheduler holds, each a batch and its token, and the CPU
    /// limit each worker gets now. Never fails: what goes wrong is logged and tried again next
    /// look.
    pub async fn look(&self, held: &[(Uuid, Uuid)], nano_cpus: i64) {
        let held_ids: HashSet<Uuid> = held.iter().map(|(batch_id, _token)| *batch_id).collect();
        let mut ready: Vec<(Uuid, bool)> = Vec::new();
        // Every worker still running, including those stopping: what they hold in Docker is
        // theirs to remove or leave, never the leftovers' sweep's.
        let mut alive: HashSet<Uuid> = HashSet::new();
        {
            let mut running = lock(&self.inner.running);
            for (batch_id, worker) in running.iter() {
                alive.insert(*batch_id);
                if !held_ids.contains(batch_id) {
                    // The slot was paused or cancelled before the worker started, another engine
                    // took it over, or the worker just finished it. A worker still at work stops
                    // and leaves its container; it is forgotten once its task ends.
                    worker.stop.cancel();
                }
            }
            for (batch_id, token) in held {
                match running.get_mut(batch_id) {
                    Some(worker) if worker.token == *token => {
                        let resize = worker.nano_cpus != nano_cpus;
                        worker.nano_cpus = nano_cpus;
                        if worker.ready.load(Ordering::Acquire) {
                            ready.push((*batch_id, resize));
                        }
                    }
                    _replaced_or_new => {
                        if let Some(stale) = running.remove(batch_id) {
                            stale.stop.cancel();
                        }
                        running.insert(*batch_id, self.spawn(*batch_id, *token, nano_cpus));
                    }
                }
            }
        }

        if !ready.is_empty() {
            match self.inner.deps.containers.api_replicas().await {
                Ok(replicas) => {
                    for (batch_id, resize) in ready {
                        self.keep_up(batch_id, &replicas, resize, nano_cpus).await;
                    }
                }
                Err(error) => event!(
                    name: "worker_pool.workers.replicas_failure",
                    Level::WARN,
                    error.message = %format!("{error:#}"),
                    "could not find the API's replicas to join to workers' networks",
                ),
            }
        }
        alive.extend(held_ids);
        self.remove_leftovers(&alive).await;
    }

    /// Stops every worker, which interrupts the turns under way and leaves their containers for
    /// the next engine, and waits briefly for each to record how its turn ended.
    pub async fn shutdown(&self) {
        self.inner.shutdown.cancel();
        self.inner.tracker.close();
        if tokio::time::timeout(SHUTDOWN_GRACE, self.inner.tracker.wait())
            .await
            .is_err()
        {
            event!(
                name: "worker_pool.workers.shutdown_timeout",
                Level::WARN,
                "workers were still recording when the grace period ended",
            );
        }
    }

    fn spawn(&self, batch_id: Uuid, token: Uuid, nano_cpus: i64) -> Running {
        let stop = self.inner.shutdown.child_token();
        let ready = Arc::new(AtomicBool::new(false));
        let worker = Worker {
            deps: self.inner.deps.clone(),
            batch_id,
            token,
            stop: stop.clone(),
            ready: Arc::clone(&ready),
            nano_cpus,
        };
        let workers = self.clone();
        self.inner.tracker.spawn(async move {
            run::run(worker).await;
            workers.forget(batch_id, token);
        });
        Running {
            token,
            stop,
            ready,
            nano_cpus,
        }
    }

    /// Forgets the worker of `batch_id` started under `token`, once it ended.
    fn forget(&self, batch_id: Uuid, token: Uuid) {
        let mut running = lock(&self.inner.running);
        if running
            .get(&batch_id)
            .is_some_and(|worker| worker.token == token)
        {
            running.remove(&batch_id);
        }
    }

    /// Joins the API to a running worker's network again, which compose drops when it recreates
    /// the API, and gives its container the pool's current CPU share when it changed.
    async fn keep_up(&self, batch_id: Uuid, replicas: &[String], resize: bool, nano_cpus: i64) {
        let containers = &self.inner.deps.containers;
        if let Err(error) = containers.ensure_network(batch_id, replicas).await {
            event!(
                name: "worker_pool.worker.network_failure",
                Level::WARN,
                work_batch.id = %batch_id,
                error.message = %format!("{error:#}"),
                "could not join the API to a worker's network; its tool calls fail until it is",
            );
        }
        if !resize {
            return;
        }
        if let Err(error) = containers
            .workspace(batch_id, nano_cpus)
            .set_cpu_limit(nano_cpus)
            .await
        {
            event!(
                name: "worker_pool.worker.resize_failure",
                Level::WARN,
                work_batch.id = %batch_id,
                error.message = %error,
                "could not change a worker's CPU limit; it keeps the one it had",
            );
        }
    }

    /// Removes what workers left in Docker for batches no worker runs any more: a batch that
    /// ended while its engine stopped before removing its container, or one whose worker failed
    /// to remove it. A batch with a worker here (`ours`), or that another engine may still hold
    /// (`assigned` or `running`), is left alone.
    async fn remove_leftovers(&self, ours: &HashSet<Uuid>) {
        let containers = &self.inner.deps.containers;
        let removed = async {
            let found = containers.leftovers().await?;
            let unheld: Vec<Uuid> = found.difference(ours).copied().collect();
            if unheld.is_empty() {
                return anyhow::Ok(());
            }
            let mut connection = self.inner.deps.database.get().await?;
            let live: HashSet<Uuid> = batch_worker::states(&mut connection, &unheld)
                .await?
                .into_iter()
                .filter(|(_id, state)| matches!(state, BatchState::Assigned | BatchState::Running))
                .map(|(id, _state)| id)
                .collect();
            drop(connection);
            for batch_id in unheld.into_iter().filter(|id| !live.contains(id)) {
                containers.remove(batch_id).await?;
            }
            Ok(())
        };
        if let Err(error) = removed.await {
            event!(
                name: "worker_pool.leftovers.failure",
                Level::WARN,
                error.message = %format!("{error:#}"),
                "could not remove what workers left behind; looking again next time",
            );
        }
    }
}

/// Every critical section leaves the map whole, so a poisoned lock holds nothing half done.
fn lock<Value>(mutex: &Mutex<Value>) -> std::sync::MutexGuard<'_, Value> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
