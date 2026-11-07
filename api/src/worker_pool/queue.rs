// Copyright © 2026 Jalapeno Labs

//! The work queue as people and Elysia read it: the pool, the batches holding its slots, every
//! batch waiting, in the order the pool will hand them slots, and every batch proposed, which
//! waits for a person's approval before it is any part of that order.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use uuid::Uuid;

use super::order::{self, Waiting};
use crate::agent_configuration::{Hold, hold};
use crate::models::work_batch::{self, BatchState, WorkBatch};
use crate::models::worker_pool;
use crate::models::{agent_configuration, user};

/// The most waiting batches read, oldest first, and apart from them the most proposed ones. A
/// queue longer than this is far more work than the pool gets through in a day; the newest wait
/// unread until the oldest move on. Proposals are read on their own, so however many wait for a
/// person, they never crowd approved work out of what the scheduler reads.
pub const MAX_WAITING: i64 = 1_000;

/// The pool as the queue shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pool {
    /// How many workers run at once; `None` before an engine has looked, when an admin chose no
    /// size, since only the engine knows its CPU cores.
    pub size: Option<usize>,
    /// The size an admin chose, `None` to follow the default.
    pub configured_size: Option<i32>,
    /// The default the engine derives from its CPU cores, once it has looked.
    pub default_size: Option<usize>,
    pub cpu_count: Option<i32>,
    /// How many slots are in use.
    pub in_use: usize,
    /// While every Claude account is at its usage limit, when the first resets.
    pub usage_held_until: Option<DateTime<Utc>>,
    /// While no worker may start without the configuration repository, why
    /// (`crate::agent_configuration`).
    pub configuration_hold: Option<Hold>,
}

/// The work queue at a moment.
#[derive(Debug, Clone)]
pub struct Queue {
    pub pool: Pool,
    /// The batches holding slots, in the order they were handed out.
    pub in_slots: Vec<WorkBatch>,
    /// Every batch that may be handed a slot now, first first.
    pub next: Vec<WorkBatch>,
    /// Scheduled batches not yet due, soonest first.
    pub scheduled: Vec<WorkBatch>,
    /// Paused batches, oldest first.
    pub paused: Vec<WorkBatch>,
    /// Batches waiting for a person's approval, oldest first. The pool hands them nothing.
    pub proposed: Vec<WorkBatch>,
}

/// Reads the work queue at `now`.
///
/// # Errors
/// Propagates any database error.
pub async fn read(
    connection: &mut AsyncPgConnection,
    now: DateTime<Utc>,
) -> diesel::QueryResult<Queue> {
    let settings = user::workspace_settings(connection).await?;
    let configuration = agent_configuration::find(connection).await?;
    let seen = worker_pool::find(connection).await?;
    let in_slots = work_batch::in_slots(connection).await?;
    let waiting = work_batch::in_states(
        connection,
        &[BatchState::Queued, BatchState::Paused],
        MAX_WAITING,
    )
    .await?;
    let proposed = work_batch::in_states(connection, &[BatchState::Proposed], MAX_WAITING).await?;

    let cpu_count = seen.map(|pool| pool.cpu_count);
    let cores = cpu_count.and_then(|count| usize::try_from(count).ok());
    let size = match cores {
        Some(cores) => Some(super::size(settings.worker_pool_size, cores)),
        // Before an engine has looked, only an admin's size is known.
        None => settings
            .worker_pool_size
            .and_then(|size| usize::try_from(size).ok()),
    };
    let pool = Pool {
        size,
        configured_size: settings.worker_pool_size,
        default_size: cores.map(super::default_size),
        cpu_count,
        in_use: in_slots.len(),
        usage_held_until: seen
            .and_then(|pool| pool.usage_held_until)
            .filter(|until| *until > now),
        configuration_hold: hold(&settings, &configuration),
    };

    let mut by_slots: HashMap<Uuid, usize> = HashMap::new();
    for batch in &in_slots {
        *by_slots.entry(batch.project_id).or_default() += 1;
    }
    let mut queued: Vec<Waiting> = Vec::with_capacity(waiting.len());
    let mut scheduled = Vec::new();
    let mut paused = Vec::new();
    let mut by_id: HashMap<Uuid, WorkBatch> = HashMap::with_capacity(waiting.len());
    for batch in waiting {
        if batch.state == BatchState::Paused {
            paused.push(batch);
            continue;
        }
        if !batch.is_due(now) {
            scheduled.push(batch);
            continue;
        }
        queued.push(Waiting::from(&batch));
        by_id.insert(batch.id, batch);
    }
    scheduled.sort_by_key(|batch| (batch.scheduled_for, batch.id));
    let next = order::order(&queued, &by_slots, now)
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect();
    Ok(Queue {
        pool,
        in_slots,
        next,
        scheduled,
        paused,
        proposed,
    })
}
