// Copyright © 2026 Jalapeno Labs

//! `/api/v1/learnings`: what people and their agents learned, written for the next agent.
//!
//! Every route answers the company's members alone ([`MemberUser`]): learnings are part of the
//! knowledge base, which a guest never reaches (`docs/knowledge.md`, Who sees what). Anyone else
//! may save, correct, delete, restore, and undo any learning; only the feed is for admins.
//!
//! Saving and deleting are also the MCP server's `learning_save` and `learning_delete`, which
//! call [`save`] and [`delete`] with the client acting for the person, so a learning saved by an
//! agent is validated, recorded, indexed, and announced exactly as one saved here.

mod correct_learning;
mod delete_learning;
mod get_learning;
mod learning_feed;
mod list_history;
mod list_learnings;
mod restore_learning;
mod save_learning;
#[cfg(test)]
mod tests;
mod undo_change;

use axum::Router;
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

pub use self::delete_learning::delete;
pub use self::save_learning::{SaveLearningRequest, save};
use crate::auth::MemberUser;
use crate::models::learning::{Agent, Author, HistoryEntry, Learning, LearningCategory, Recorded};
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_learnings::handle).post(save_learning::handle))
        .route("/feed", get(learning_feed::handle))
        .route(
            "/{id}",
            get(get_learning::handle)
                .patch(correct_learning::handle)
                .delete(delete_learning::handle),
        )
        .route("/{id}/restore", post(restore_learning::handle))
        .route("/{id}/history", get(list_history::handle))
        .route("/{id}/history/{entry_id}/undo", post(undo_change::handle))
}

/// The agent that acted for someone, as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResponse {
    /// Its recorded form: `mcp:<client id>` for an MCP client.
    id: String,
    /// What it called itself, if anything.
    name: Option<String>,
}

impl AgentResponse {
    fn new(id: Option<String>, name: Option<String>) -> Option<Self> {
        id.map(|id| Self { id, name })
    }
}

/// A learning as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningResponse {
    id: Uuid,
    title: String,
    body: String,
    category: LearningCategory,
    evidence: String,
    /// The agent that acted for its creator when it was first saved; `null` when they saved it
    /// themselves.
    agent: Option<AgentResponse>,
    /// Who first saved it. Who changed it since is in its history.
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    /// Set while it is deleted; it can be restored.
    deleted_at: Option<DateTime<Utc>>,
}

impl From<Learning> for LearningResponse {
    fn from(learning: Learning) -> Self {
        Self {
            id: learning.id,
            title: learning.title,
            body: learning.body,
            category: learning.category,
            evidence: learning.evidence,
            agent: AgentResponse::new(learning.agent, learning.agent_name),
            created_by: learning.created_by,
            created_at: learning.created_at,
            updated_at: learning.updated_at,
            deleted_at: learning.deleted_at,
        }
    }
}

/// A history entry as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntryResponse {
    id: Uuid,
    learning_id: Uuid,
    /// `created`, `revised`, `deleted`, or `restored`.
    kind: String,
    /// Who made the change.
    created_by: Uuid,
    /// The agent that acted for them, if one did.
    agent: Option<AgentResponse>,
    /// Shaped by `kind`; see `docs/knowledge.md`, History.
    data: Value,
    created_at: DateTime<Utc>,
}

impl From<HistoryEntry> for HistoryEntryResponse {
    fn from(entry: HistoryEntry) -> Self {
        Self {
            id: entry.id,
            learning_id: entry.learning_id,
            kind: entry.kind,
            created_by: entry.created_by,
            agent: AgentResponse::new(entry.agent, entry.agent_name),
            data: entry.data,
            created_at: entry.created_at,
        }
    }
}

/// Who a write through these routes is by: the member, with the agent acting for them, now.
fn author<'a>(member: &MemberUser, agent: Option<&'a Agent>) -> Author<'a> {
    Author {
        user_id: member.id(),
        agent,
        at: Utc::now(),
    }
}

/// Announces a write that changed something, and answers the learning it left.
fn announce(state: &AppState, recorded: Recorded) -> LearningResponse {
    if !recorded.history.is_empty() {
        state.events.publish(&ServerEvent::LearningChanged {
            id: recorded.learning.id,
        });
    }
    LearningResponse::from(recorded.learning)
}
