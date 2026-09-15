// Copyright © 2026 Jalapeno Labs

//! `/api/v1/coding-sessions`: Arsox threads Elysium opened, and the turns sent to them.
//!
//! Each call that touches a thread goes through the fleet's client for the session's
//! satellite. A satellite failure answers `502` with the satellite's own message.

mod create_coding_session;
mod delete_coding_session;
mod list_coding_sessions;
mod list_session_events;
mod model_stack;
mod rename_coding_session;
mod start_turn;

use arsox_sdk::proto::common::v1::{
    CostCeiling, Duration, DurationCeiling, Money, TokenCeiling, Unlimited, cost_ceiling,
    duration_ceiling, token_ceiling,
};
use arsox_sdk::proto::harness::v1::Harness;
use arsox_sdk::proto::settings::v1::{Budget, Repo, ThreadSettings};
use axum::Router;
use axum::routing::{get, patch, post};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;
use validator::ValidationError;

use self::model_stack::ModelStack;
use crate::fleet::views::ThreadStatus;
use crate::models::coding_session::CodingSession;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(list_coding_sessions::handle).post(create_coding_session::handle),
        )
        .route(
            "/{id}",
            patch(rename_coding_session::handle).delete(delete_coding_session::handle),
        )
        .route("/{id}/events", get(list_session_events::handle))
        .route("/{id}/turns", post(start_turn::handle))
}

/// How long a thread may sit idle before its satellite collects it and its workspace.
/// A week covers a session left open over a weekend; the satellite requires a value
/// so a forgotten thread cannot hold a disk forever.
const THREAD_IDLE_TTL_SECONDS: i64 = 7 * 24 * 60 * 60;

/// Spend ceiling for one thread across all its turns, in whole US dollars. The
/// satellite refuses further model calls past it; raise it here for longer sessions.
const THREAD_COST_CEILING_DOLLARS: i64 = 25;

/// Longest a single turn may run before the satellite stops it.
const TURN_WALL_CLOCK_CEILING_SECONDS: i64 = 60 * 60;

/// A session as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionResponse {
    id: Uuid,
    project_id: Uuid,
    satellite_id: Uuid,
    thread_id: String,
    title: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    /// The thread as of the fleet's latest poll; `None` until the first poll sees it.
    thread: Option<ThreadStatus>,
}

impl CodingSessionResponse {
    pub fn new(session: CodingSession, thread: Option<ThreadStatus>) -> Self {
        Self {
            id: session.id,
            project_id: session.project_id,
            satellite_id: session.satellite_id,
            thread_id: session.thread_id,
            title: session.title,
            created_at: session.created_at,
            updated_at: session.updated_at,
            thread,
        }
    }
}

/// Settings for a new thread: Elysium's policy ceilings, the optional repository, and
/// the credentials the thread fails over through.
///
/// Without a stack the thread declares no endpoint, and the satellite falls back to
/// whatever credential it holds itself.
fn thread_settings(repository: Option<Repo>, stack: Option<ModelStack>) -> ThreadSettings {
    let budget = Budget {
        // Per-turn tokens are bounded by the cost and wall clock ceilings instead.
        max_tokens_per_turn: Some(TokenCeiling {
            ceiling: Some(token_ceiling::Ceiling::Unlimited(Unlimited {})),
        }),
        max_cost_per_thread: Some(CostCeiling {
            ceiling: Some(cost_ceiling::Ceiling::Cost(Money::usd(
                THREAD_COST_CEILING_DOLLARS,
                0,
            ))),
        }),
        max_wall_clock_per_turn: Some(DurationCeiling {
            ceiling: Some(duration_ceiling::Ceiling::Duration(Duration {
                seconds: TURN_WALL_CLOCK_CEILING_SECONDS,
                nanos: 0,
            })),
        }),
    };

    // The harness has to match the endpoints: a Claude list cannot drive Codex, and
    // both travel together out of the stack for that reason.
    let (harness, models) = stack.map_or_else(
        || (Harness::Unspecified, Vec::new()),
        |stack| (stack.harness, stack.endpoints),
    );

    ThreadSettings {
        idle_ttl: Some(Duration {
            seconds: THREAD_IDLE_TTL_SECONDS,
            nanos: 0,
        }),
        budget: Some(budget),
        harness: harness.into(),
        models,
        repos: repository.into_iter().collect(),
        ..ThreadSettings::default()
    }
}

/// Rejects text that is only whitespace; `length` alone would accept `"   "`.
fn validate_not_blank(value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::new("blank").with_message("must not be blank".into()));
    }
    Ok(())
}
