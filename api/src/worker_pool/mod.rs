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
//!   engine renews, which a worker (#85) takes by starting the batch and gives back by finishing
//!   it (`crate::models::work_batch`, Slots and their leases). Leases are taken over after a
//!   restart, so no slot is lost and none is held twice.
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

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

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

/// Starts the scheduler, which hands out the pool's slots until `shutdown`, then lets the leases
/// it holds lapse for the next engine. Whatever frees a slot or queues work, in any process,
/// sends `Signal::WorkerPool`, so it looks at once.
pub fn start(database: Pool, events: EventBus, shutdown: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(scheduler::run(database, events, cpu_count(), shutdown))
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
