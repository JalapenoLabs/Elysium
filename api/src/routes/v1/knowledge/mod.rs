// Copyright © 2026 Jalapeno Labs

//! `/api/v1/knowledge`: searching the knowledge base, and opening what a search cites.
//!
//! Every route answers the company's members alone ([`crate::auth::MemberUser`]): a guest never
//! reaches the knowledge base (`docs/knowledge.md`, Who sees what). Within it nothing is limited by
//! whose record a document came from; the `person` filter only narrows.
//!
//! The search is [`crate::knowledge::search::KnowledgeSearch::search`] and a document
//! [`crate::models::knowledge_document::get`], which the MCP server's tools call too, so every
//! caller finds and opens the same things.

mod get_document;
mod search_knowledge;
#[cfg(test)]
mod tests;

use axum::Router;
use axum::routing::get;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/search", get(search_knowledge::handle))
        .route("/documents/{id}", get(get_document::handle))
}
