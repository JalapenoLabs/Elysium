// Copyright © 2026 Jalapeno Labs

//! `/api/v1/knowledge/asks`: questions asked of the knowledge base for a written answer.
//!
//! Asking ([`ask`]) searches as the asker, records what the search offered, and leaves the answer
//! to the engine's answerer (`crate::knowledge::answerer`), which takes seconds to tens of
//! seconds: the route answers at once with the ask, and `knowledgeAsk.changed` says when it is
//! answered. The MCP server's `knowledge_ask` calls the same [`ask`], then waits a bounded time
//! for the answer ([`wait_for_answer`]), within the request timeout every route has.
//!
//! An ask is its asker's alone: the routes answer `404` for anyone else's. Every route answers
//! the company's members alone ([`crate::auth::MemberUser`]), as the knowledge base does.

mod create_ask;
mod get_ask;
mod list_asks;
#[cfg(test)]
mod tests;

use axum::Router;
use axum::routing::get;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

pub use self::create_ask::{AskKnowledgeRequest, ask};
pub use self::get_ask::{AskArguments, wait_for_answer};
use crate::models::knowledge_ask::{KnowledgeAsk, KnowledgeAskState};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_asks::handle).post(create_ask::handle))
        .route("/{id}", get(get_ask::handle))
}

/// An ask as its asker sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeAskResponse {
    pub id: Uuid,
    pub question: String,
    /// What the search was narrowed to: `sources`, `project`, `person`, `from`, `to`, each only
    /// when asked for.
    pub filters: Value,
    pub state: KnowledgeAskState,
    /// Once answered: `claims`, each with its `text` and the `sources` it cites by number;
    /// `sources`, each a `number`, its `citation`, and the `position` of the passage cited; and
    /// `unsupportedClaims`, how many statements were left out for citing no passage.
    pub answer: Option<Value>,
    /// Why there is no answer, once it failed.
    pub error: Option<String>,
    /// Whether the search ranked by meaning as well as by words.
    pub searched_by_meaning: bool,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl From<KnowledgeAsk> for KnowledgeAskResponse {
    fn from(ask: KnowledgeAsk) -> Self {
        Self {
            id: ask.id,
            question: ask.question,
            filters: ask.filters,
            state: ask.state,
            answer: ask.answer,
            error: ask.error,
            searched_by_meaning: ask.searched_by_meaning,
            created_at: ask.created_at,
            started_at: ask.started_at,
            finished_at: ask.finished_at,
        }
    }
}
