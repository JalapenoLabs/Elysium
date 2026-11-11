// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/work-queue`: the worker pool and every batch holding a slot or waiting for one,
//! in the order the pool hands them slots, and every batch proposed, waiting for a person, each
//! with what its chain starts from (`crate::models::work_batch::why`). Members only, as every
//! batch's route is. See `crate::worker_pool::queue` and `docs/engine.md`, Workers and the pool.

use std::collections::HashMap;

use anyhow::Context;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::agent_configuration::Hold;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::work_batch::lineage::Origin;
use crate::models::work_batch::why::{self, Link, Why};
use crate::models::work_batch::{BatchState, Priority, WorkBatch};
use crate::state::AppState;
use crate::worker_pool::queue::{self, Pool, Queue};

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(handle))
}

async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let queue = queue::read(&mut connection, Utc::now()).await?;
    // Every list's chains in one read, rather than a read per row.
    let listed: Vec<&WorkBatch> = [
        &queue.in_slots,
        &queue.next,
        &queue.scheduled,
        &queue.paused,
        &queue.proposed,
    ]
    .into_iter()
    .flatten()
    .collect();
    let whys = why::read_many(&mut connection, &listed).await?;
    Ok(Json(
        json!({ "workQueue": QueueResponse::new(queue, &whys) }),
    ))
}

/// The work queue as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueResponse {
    pool: PoolResponse,
    /// The batches holding slots, in the order they were handed out.
    in_slots: Vec<EntryResponse>,
    /// Every batch the pool may hand a slot now, first first.
    next: Vec<EntryResponse>,
    /// Scheduled batches not yet due, soonest first.
    scheduled: Vec<EntryResponse>,
    paused: Vec<EntryResponse>,
    /// Batches waiting for a person's approval, oldest first; the pool hands them nothing.
    proposed: Vec<EntryResponse>,
}

impl QueueResponse {
    /// `queue` as clients see it, each batch with the head of its chain in `whys`.
    fn new(queue: Queue, whys: &HashMap<Uuid, Why>) -> Self {
        let entries = |batches: Vec<WorkBatch>| {
            batches
                .into_iter()
                .map(|batch| {
                    let cause = whys.get(&batch.id).and_then(Why::head).cloned();
                    EntryResponse::new(batch, cause)
                })
                .collect()
        };
        Self {
            pool: PoolResponse::from(queue.pool),
            in_slots: entries(queue.in_slots),
            next: entries(queue.next),
            scheduled: entries(queue.scheduled),
            paused: entries(queue.paused),
            proposed: entries(queue.proposed),
        }
    }
}

/// The pool as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolResponse {
    /// How many workers run at once; `null` until the engine has looked, when no admin chose.
    size: Option<usize>,
    /// The size an admin chose; `null` follows the default.
    configured_size: Option<i32>,
    /// One per CPU core the engine sees minus two, at least one.
    default_size: Option<usize>,
    cpu_count: Option<i32>,
    in_use: usize,
    /// While every Claude account is at its usage limit, when the first resets.
    usage_held_until: Option<DateTime<Utc>>,
    /// While no worker may start without the configuration repository, why: `{ reason:
    /// 'unset' | 'fetching' | 'failed', error?, failedAt? }`.
    configuration_hold: Option<Hold>,
}

impl From<Pool> for PoolResponse {
    fn from(pool: Pool) -> Self {
        Self {
            size: pool.size,
            configured_size: pool.configured_size,
            default_size: pool.default_size,
            cpu_count: pool.cpu_count,
            in_use: pool.in_use,
            usage_held_until: pool.usage_held_until,
            configuration_hold: pool.configuration_hold,
        }
    }
}

/// A batch as the queue lists it: what tells it apart, without its goal, which its own page
/// reads.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryResponse {
    id: Uuid,
    project_id: Uuid,
    title: String,
    priority: Priority,
    scheduled_for: Option<DateTime<Utc>>,
    state: BatchState,
    pinned_at: Option<DateTime<Utc>>,
    assigned_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    /// `human_rooted` or `found`.
    origin: Origin,
    /// How many batches led to it.
    generation: i32,
    /// What its chain starts from: the first thing that asked for it, or the first item it works
    /// on, as `GET /work-batches/{id}` names it in `why.chain`; `null` for one that names nothing.
    cause: Option<Link>,
}

impl EntryResponse {
    fn new(batch: WorkBatch, cause: Option<Link>) -> Self {
        Self {
            id: batch.id,
            project_id: batch.project_id,
            title: batch.title,
            priority: batch.priority,
            scheduled_for: batch.scheduled_for,
            state: batch.state,
            pinned_at: batch.pinned_at,
            assigned_at: batch.assigned_at,
            created_at: batch.created_at,
            started_at: batch.started_at,
            origin: batch.origin,
            generation: batch.generation,
            cause,
        }
    }
}
