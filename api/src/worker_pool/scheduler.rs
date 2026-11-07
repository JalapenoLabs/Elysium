// Copyright © 2026 Jalapeno Labs

//! The scheduler: it hands the pool's free slots to waiting batches, and holds the slots it
//! handed out.
//!
//! It looks when it is signalled ([`Signal::WorkerPool`]), when the next scheduled batch comes
//! due, when a usage hold ends, and every [`LOOK_INTERVAL`] otherwise. Each look:
//!
//! 1. renews the leases of the slots it holds, letting go of any paused or cancelled since, and
//!    takes over every slot whose lease lapsed: those of an engine that died, and those the last
//!    engine released as it stopped;
//! 2. records the CPU cores it sees and whether every Claude account is at its usage limit,
//!    which the work queue's page shows (`crate::models::worker_pool`);
//! 3. reads the queue ([`super::queue`]) and, when the pool is over its size, takes back the slots
//!    handed out last that no worker has started on;
//! 4. unless a usage limit holds the pool, hands each free slot to the next batch in order.
//!
//! After each look the workers (`super::worker`) look too: a worker for every slot held, none
//! for a slot lost, each at the pool's CPU share.
//!
//! As it starts, it raises the approval item of any project whose proposals never had one.
//!
//! At shutdown it lets the leases it holds lapse, so the next engine takes them over as it
//! starts, and stops its workers, leaving their containers for that engine.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use super::queue;
use super::worker::Workers;
use crate::database::Pool;
use crate::models::work_batch::{self, BatchState, Slot, approval_item};
use crate::models::worker_pool;
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::routes::v1::action_items::publish_item_write;
use crate::runner::credentials;
use crate::worker_pool::actions::announce;

/// How often the scheduler looks without a reason to: four times per lease, so a renewal or two
/// lost to a slow database never lets a held slot lapse, and a lost signal costs seconds.
const LOOK_INTERVAL: Duration = Duration::from_secs(15);

/// The scheduler's state between looks: the slots it holds, each batch's id and the token
/// fencing its slot, and the pool's size as it last read it.
struct Scheduler {
    database: Pool,
    events: EventBus,
    cpu_count: i32,
    held: HashMap<Uuid, Uuid>,
    size: Option<usize>,
}

/// Hands out the pool's slots until `shutdown`, on a host with `cpu_count` cores, and keeps a
/// worker running for each slot it holds.
pub(super) async fn run(
    database: Pool,
    events: EventBus,
    workers: Workers,
    cpu_count: usize,
    shutdown: CancellationToken,
) {
    // Taken before the first look, so a signal sent during any look wakes the next.
    let mut wake = events.wake_on(|signal| *signal == Signal::WorkerPool);
    let mut scheduler = Scheduler {
        database,
        events,
        cpu_count: i32::try_from(cpu_count).unwrap_or(i32::MAX),
        held: HashMap::new(),
        size: None,
    };
    scheduler.tell_of_proposals().await;
    loop {
        let next_look = match scheduler.look().await {
            Ok(due) => due
                .and_then(|at| (at - Utc::now()).to_std().ok())
                .map_or(LOOK_INTERVAL, |until| until.min(LOOK_INTERVAL)),
            Err(error) => {
                event!(
                    name: "worker_pool.look.failure",
                    Level::ERROR,
                    error.message = %format!("{error:#}"),
                    "could not look at the worker pool; looking again shortly",
                );
                LOOK_INTERVAL
            }
        };
        let held: Vec<(Uuid, Uuid)> = scheduler
            .held
            .iter()
            .map(|(batch_id, token)| (*batch_id, *token))
            .collect();
        let size = scheduler
            .size
            .unwrap_or_else(|| super::default_size(cpu_count));
        workers
            .look(&held, super::worker_nano_cpus(cpu_count, size))
            .await;
        tokio::select! {
            () = shutdown.cancelled() => break,
            () = wake.woken() => {}
            () = tokio::time::sleep(next_look) => {}
        }
    }
    scheduler.release().await;
    workers.shutdown().await;
    event!(
        name: "worker_pool.scheduler.stopped",
        Level::INFO,
        "the worker pool's scheduler stopped",
    );
}

impl Scheduler {
    /// One look. Answers when the next is owed besides the interval: the next scheduled batch's
    /// time, or a usage hold's end.
    async fn look(&mut self) -> anyhow::Result<Option<DateTime<Utc>>> {
        let mut connection = self.database.get().await?;
        let now = Utc::now();

        let held: Vec<(Uuid, Uuid)> = self.held.iter().map(|(id, token)| (*id, *token)).collect();
        let still_held = work_batch::renew(&mut connection, &held, now).await?;
        self.held.retain(|id, _token| still_held.contains(id));
        for slot in work_batch::adopt(&mut connection, now).await? {
            event!(
                name: "worker_pool.slot.adopted",
                Level::INFO,
                work_batch.id = %slot.batch.id,
                work_batch.state = ?slot.batch.state,
                "took over a slot whose engine stopped holding it",
            );
            self.held.insert(slot.batch.id, slot.token);
        }

        let usage_held_until = credentials::usage_hold(&mut connection, now).await?;
        if worker_pool::record(&mut connection, self.cpu_count, usage_held_until).await? {
            self.events.publish(&ServerEvent::WorkerPoolChanged);
        }

        let waiting = queue::read(&mut connection, now).await?;
        let next_due = waiting
            .scheduled
            .first()
            .and_then(|batch| batch.scheduled_for);
        self.size = waiting.pool.size;
        let Some(size) = waiting.pool.size else {
            return Ok(next_due);
        };
        if waiting.pool.in_use > size {
            self.shrink(
                &mut connection,
                &waiting.in_slots,
                waiting.pool.in_use - size,
            )
            .await?;
            return Ok(next_due);
        }
        if let Some(resets_at) = usage_held_until {
            event!(
                name: "worker_pool.usage_held",
                Level::DEBUG,
                usage.resets_at = %resets_at,
                "every Claude account is at its usage limit; no slot is handed out until a reset",
            );
            return Ok(Some(next_due.map_or(resets_at, |due| due.min(resets_at))));
        }
        if waiting.pool.in_use < size && !waiting.next.is_empty() {
            let ids: Vec<Uuid> = waiting.next.iter().map(|batch| batch.id).collect();
            let handed_out = work_batch::assign(&mut connection, &ids, size, now).await?;
            self.took(&handed_out);
        }
        Ok(next_due)
    }

    /// Records the slots just handed out as held, and announces each.
    fn took(&mut self, slots: &[Slot]) {
        for slot in slots {
            event!(
                name: "worker_pool.slot.assigned",
                Level::INFO,
                work_batch.id = %slot.batch.id,
                project.id = %slot.batch.project_id,
                "handed a slot to a batch",
            );
            self.held.insert(slot.batch.id, slot.token);
            announce(&self.events, &slot.batch);
        }
    }

    /// Takes `excess` slots back from the batches handed one last that no worker has started on,
    /// returning them to the queue in their places. A running batch keeps its slot: its worker
    /// finishes, and the pool shrinks as slots come back.
    async fn shrink(
        &mut self,
        connection: &mut diesel_async::AsyncPgConnection,
        in_slots: &[work_batch::WorkBatch],
        excess: usize,
    ) -> anyhow::Result<()> {
        let newest_unstarted = in_slots
            .iter()
            .rev()
            .filter(|batch| batch.state == BatchState::Assigned)
            .filter_map(|batch| {
                let token = self.held.get(&batch.id)?;
                Some(Slot {
                    batch: batch.clone(),
                    token: *token,
                })
            })
            .take(excess);
        for slot in newest_unstarted.collect::<Vec<_>>() {
            let Some(queued) = work_batch::unassign(connection, &slot).await? else {
                continue;
            };
            event!(
                name: "worker_pool.slot.taken_back",
                Level::INFO,
                work_batch.id = %queued.id,
                "the pool shrank below the slots in use; a batch no worker started waits again",
            );
            self.held.remove(&queued.id);
            announce(&self.events, &queued);
        }
        Ok(())
    }

    /// Raises the approval item of every project whose proposals never had one
    /// (`work_batch::approval_item::reconcile`), so no proposal waits unseen, and announces each.
    /// Once at start: after that, every write that proposes a batch keeps its project's item. One
    /// that fails is logged, and the next start tries again; the gate holds either way.
    async fn tell_of_proposals(&self) {
        let reconciled = async {
            let mut connection = self.database.get().await?;
            let now = Utc::now();
            let raised = approval_item::reconcile(&mut connection, now).await?;
            for item in raised {
                let item_id = item.record.id;
                if let Err(error) =
                    publish_item_write(&self.events, &mut connection, item, &[], now).await
                {
                    event!(
                        name: "worker_pool.approval_item.unpublished",
                        Level::WARN,
                        action_item.id = %item_id,
                        error.message = %error,
                        "a project's approval item was raised but could not be announced",
                    );
                }
            }
            anyhow::Ok(())
        };
        if let Err(error) = reconciled.await {
            event!(
                name: "worker_pool.approval_items.reconcile_failure",
                Level::WARN,
                error.message = %format!("{error:#}"),
                "could not raise the approval items proposals were missing; the next start tries again",
            );
        }
    }

    /// Lets every lease it holds lapse, for the next engine to take over at once.
    async fn release(&mut self) {
        let held: Vec<(Uuid, Uuid)> = self.held.drain().collect();
        if held.is_empty() {
            return;
        }
        let released = async {
            let mut connection = self.database.get().await?;
            anyhow::Ok(work_batch::release(&mut connection, &held, Utc::now()).await?)
        };
        if let Err(error) = released.await {
            event!(
                name: "worker_pool.release.failure",
                Level::WARN,
                error.message = %format!("{error:#}"),
                "could not let go of the pool's slots; the next engine takes them once their leases lapse",
            );
        }
    }
}

#[cfg(test)]
mod tests;
