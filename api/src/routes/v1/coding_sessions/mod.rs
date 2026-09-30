// Copyright © 2026 Jalapeno Labs

//! `/api/v1/coding-sessions`: Arsox threads Elysium opened, and the turns sent to them.
//!
//! Each call that touches a thread goes through the fleet's client for the session's
//! satellite. A satellite failure answers `502` with the satellite's own message.

mod create_coding_session;
mod delete_coding_session;
mod first_turn;
mod github_token;
mod list_coding_sessions;
mod list_session_events;
pub mod model_stack;
pub mod open;
mod rename_coding_session;
mod start_turn;

use arsox_sdk::proto::common::v1::Secret;
use arsox_sdk::proto::common::v1::{
    CostCeiling, Duration, DurationCeiling, Money, TokenCeiling, Unlimited, cost_ceiling,
    duration_ceiling, token_ceiling,
};
use arsox_sdk::proto::harness::v1::Harness;
use arsox_sdk::proto::settings::v1::EnvVar;
use arsox_sdk::proto::settings::v1::{Budget, Repo, ThreadSettings};
use axum::Router;
use axum::routing::{get, patch, post};
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use uuid::Uuid;
use validator::ValidationError;

use self::model_stack::ModelStack;
use crate::blender;
use crate::errors::ApiError;
use crate::fleet::views::ThreadStatus;
use crate::models::coding_session::CodingSession;
use crate::models::environment_variable::ThreadVariable;
use crate::state::AppState;
use crate::tools;

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
    /// The session's number.
    id: i64,
    /// Always set for a Coding session; a Studio session has its item's, which is optional.
    project_id: Option<Uuid>,
    /// `null` once the satellite was deleted; the session's history is kept.
    satellite_id: Option<Uuid>,
    thread_id: String,
    title: String,
    /// The GitHub token the thread was started with, or `null` for none or a token since
    /// deleted.
    github_credential_id: Option<Uuid>,
    /// The action item the session was started from, if any.
    action_item_id: Option<Uuid>,
    /// The Studio item the session works on; `null` for a Coding session.
    studio_item_id: Option<Uuid>,
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
            github_credential_id: session.github_credential_id,
            action_item_id: session.action_item_id,
            studio_item_id: session.studio_item_id,
            created_at: session.created_at,
            updated_at: session.updated_at,
            thread,
        }
    }
}

/// What a new thread is opened with, beyond Elysium's policy ceilings.
#[derive(Debug, Default)]
pub struct ThreadPlan<'plan> {
    /// The repositories to clone, in order.
    pub repositories: Vec<Repo>,
    /// The credentials the thread fails over through. Without a stack the thread declares no
    /// endpoint, and the satellite falls back to whatever credential it holds itself.
    pub stack: Option<ModelStack>,
    /// The workspace's environment variables.
    pub variables: &'plan [ThreadVariable],
    /// The GitHub token the agent works with.
    pub github_token: Option<&'plan SecretString>,
    /// Whether the session's project reaches a storage location, which is what the storage
    /// tools would work on.
    pub has_storage_locations: bool,
    /// Whether the session has a project, which is what the work tools are scoped to. Every
    /// Coding session does; a Studio item may not.
    pub has_project: bool,
    /// Instructions written into the agent's `AGENTS.md`, below the satellite's own.
    pub instructions: String,
}

/// Settings for a new thread: Elysium's policy ceilings, the Blender services and the MCP
/// server that reaches them, and everything in `plan`. Elysium relays the work tools when the
/// session has a project, and the storage tools when it has a storage location to use them on.
pub fn thread_settings(plan: ThreadPlan<'_>) -> ThreadSettings {
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
    let (harness, models) = plan.stack.map_or_else(
        || (Harness::Unspecified, Vec::new()),
        |stack| (stack.harness, stack.endpoints),
    );

    // A thread takes its tools once. Each call re-checks what it may reach anyway.
    let storage = plan
        .has_storage_locations
        .then(|| tools::relayed_server(&tools::storage::SERVER));
    let work = plan
        .has_project
        .then(|| tools::relayed_server(&tools::work::SERVER));

    ThreadSettings {
        idle_ttl: Some(Duration {
            seconds: THREAD_IDLE_TTL_SECONDS,
            nanos: 0,
        }),
        budget: Some(budget),
        harness: harness.into(),
        models,
        repos: plan.repositories,
        github: plan.github_token.map(github_token::integration),
        env: thread_environment(plan.variables, plan.github_token),
        prompt: plan.instructions,
        // Every thread models in a Blender of its own; see `crate::blender`.
        services: blender::services(),
        mcp_servers: vec![blender::mcp_server()],
        relayed_mcp_servers: storage.into_iter().chain(work).collect(),
        ..ThreadSettings::default()
    }
}

/// The thread's environment: the workspace's variables, then the ones Elysium sets itself.
/// Elysium's names are refused as workspace variables, so neither list can override the
/// other; Elysium's go last all the same, as the satellite applies the last value for a key.
fn thread_environment(
    variables: &[ThreadVariable],
    github_token: Option<&SecretString>,
) -> Vec<EnvVar> {
    let workspace = variables.iter().map(|variable| EnvVar {
        key: variable.key.clone(),
        value: Some(Secret {
            value: Some(variable.value.expose_secret().to_owned()),
            display: None,
        }),
        is_secret: Some(variable.is_secret),
    });
    let elysium = github_token
        .map(github_token::thread_environment)
        .unwrap_or_default();
    workspace.chain(elysium).collect()
}

/// The satellite a session's thread runs on.
///
/// # Errors
/// Answers `409` for a session whose satellite was deleted: its history is kept, but there is
/// no thread left to reach.
fn satellite_of(session: &CodingSession) -> Result<Uuid, ApiError> {
    session
        .satellite_id
        .ok_or(ApiError::Conflict("the session's satellite was deleted"))
}

/// Rejects text that is only whitespace; `length` alone would accept `"   "`.
pub fn validate_not_blank(value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::new("blank").with_message("must not be blank".into()));
    }
    Ok(())
}
