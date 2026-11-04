// Copyright © 2026 Jalapeno Labs

//! The engine's worker pool: one global set of slots that Project Managers' work batches are
//! handed, a worker each. See `docs/engine.md`, Workers and the pool.
//!
//! Project Managers never start containers; they submit batches (`crate::project_managers`). The
//! engine's [`scheduler`] hands each free slot to the batch the [`order`] puts first: a batch
//! asked to run next, then due scheduled work, ASAP, and normal, oldest first within each, round
//! robin across projects. It hands out none while every Claude account is at its usage limit, and
//! looks again at the earliest reset.
//!
//! - **Size.** An admin's setting (`workspace_settings.worker_pool_size`), or by default one
//!   worker per CPU core the engine sees minus [`RESERVED_CORES`], at least one, with no maximum
//!   ([`size`]).
//! - **The hand-off.** A slot handed out is a batch `assigned` and claimed under a lease the
//!   engine renews, which a [`worker`] takes by starting the batch and gives back by finishing
//!   it (`crate::models::work_batch`, Slots and their leases). Leases are taken over after a
//!   restart, so no slot is lost and none is held twice.
//! - **Workers.** Each slot is one [`worker`]: Claude Code in a container of its own, limited to
//!   the pool's share of the host's CPU ([`worker_nano_cpus`]).
//! - **People and Elysia** run a waiting batch next, change its priority, pause it, or cancel it
//!   ([`actions`]); the routes, Elysia's tools, and a Project Manager's tools all go through the
//!   same functions, so each change is checked, recorded in the batch's history, and announced
//!   alike.
//! - [`queue`] reads the pool and every batch waiting or in a slot, in order, as the work queue's
//!   page and Elysia's tools show it.

pub mod actions;
pub mod order;
pub mod queue;
mod scheduler;
pub mod worker;

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use self::worker::Workers;
use crate::database::Pool;
use crate::realtime::EventBus;

/// The CPU cores the default size leaves free: one each for the API, the engine, and Postgres
/// is what the host needs to stay responsive while every worker builds.
pub const RESERVED_CORES: usize = 2;

/// The pool's size: an admin's `configured` size, or by default one worker per CPU core minus
/// [`RESERVED_CORES`], at least one.
pub fn size(configured: Option<i32>, cpu_count: usize) -> usize {
    configured
        .and_then(|size| usize::try_from(size).ok())
        .filter(|size| *size >= 1)
        .unwrap_or_else(|| default_size(cpu_count))
}

/// The size the pool follows unless an admin chose one, for a host with `cpu_count` cores.
pub fn default_size(cpu_count: usize) -> usize {
    cpu_count.saturating_sub(RESERVED_CORES).max(1)
}

/// The CPU cores this process may use, as the default size counts them: the cgroup's quota when
/// the container has one, and one when the count cannot be read.
pub fn cpu_count() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

/// A billionth of a CPU, as Docker's `NanoCpus` counts a container's limit.
const NANO_CPUS_PER_CPU: u64 = 1_000_000_000;

/// The smallest limit Docker accepts, a hundredth of a CPU, which an admin's pool far larger than
/// the host can reach.
const MINIMUM_NANO_CPUS: u64 = NANO_CPUS_PER_CPU / 100;

/// Each worker's CPU limit, in billionths of a CPU, for a pool of `size` on `cpu_count` cores:
/// the cores the default size leaves workers ([`RESERVED_CORES`] fewer, at least one) shared
/// evenly across every slot. So the whole pool, every slot busy, never takes more than those
/// cores, however large an admin makes it, and the API, the engine, and Postgres keep theirs. At
/// the default size each worker gets one core; a smaller pool gets more each.
pub fn worker_nano_cpus(cpu_count: usize, size: usize) -> i64 {
    let for_workers =
        u64::try_from(cpu_count.saturating_sub(RESERVED_CORES).max(1)).unwrap_or(u64::MAX);
    let slots = u64::try_from(size.max(1)).unwrap_or(u64::MAX);
    let share = for_workers.saturating_mul(NANO_CPUS_PER_CPU) / slots;
    i64::try_from(share.max(MINIMUM_NANO_CPUS)).unwrap_or(i64::MAX)
}

/// Starts the scheduler, which hands out the pool's slots until `shutdown` and keeps `workers`
/// running one per slot, then lets the leases it holds lapse for the next engine. Whatever frees
/// a slot or queues work, in any process, sends `Signal::WorkerPool`, so it looks at once.
pub fn start(
    database: Pool,
    events: EventBus,
    workers: Workers,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(scheduler::run(
        database,
        events,
        workers,
        cpu_count(),
        shutdown,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_leaves_two_cores_and_never_falls_below_one_worker() {
        assert_eq!(default_size(16), 14);
        assert_eq!(default_size(3), 1);
        assert_eq!(default_size(2), 1);
        assert_eq!(default_size(1), 1);
        assert_eq!(default_size(0), 1);
    }

    #[test]
    fn the_whole_pool_never_takes_more_than_the_cores_left_for_workers() {
        // The default size: one core each.
        assert_eq!(worker_nano_cpus(16, default_size(16)), 1_000_000_000);
        // A smaller pool shares the same fourteen cores among fewer workers.
        assert_eq!(worker_nano_cpus(16, 4), 3_500_000_000);
        // A pool larger than the host still sums to fourteen cores.
        assert_eq!(worker_nano_cpus(16, 28), 500_000_000);
        // A host with too few cores to reserve any still gives its one worker a core.
        assert_eq!(worker_nano_cpus(2, 1), 1_000_000_000);
        assert_eq!(
            worker_nano_cpus(4, 100_000),
            10_000_000,
            "never below what Docker accepts"
        );
    }

    #[test]
    fn an_admins_size_wins_with_no_maximum() {
        assert_eq!(size(Some(40), 4), 40);
        assert_eq!(size(Some(1), 64), 1);
        assert_eq!(size(None, 8), 6);
        assert_eq!(
            size(Some(0), 8),
            6,
            "a size below one, which the column refuses, is ignored"
        );
    }
}
