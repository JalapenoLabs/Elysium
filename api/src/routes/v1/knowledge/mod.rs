// Copyright © 2026 Jalapeno Labs

//! `/api/v1/knowledge`: searching the knowledge base, opening what a search cites, and listing
//! the meetings it holds.
//!
//! Every route answers the company's members alone ([`crate::auth::MemberUser`]): a guest never
//! reaches the knowledge base (`docs/knowledge.md`, Who sees what). Within it nothing is limited by
//! whose record a document came from; the `person` filter only narrows.
//!
//! Each route's work is a function of its own ([`search`], [`open`], [`list_meetings`]), which
//! the MCP server's knowledge tools and the agents' `elysium_knowledge` tools call too, so every
//! caller finds, opens, and lists the same things, checked and budgeted alike.

mod get_document;
mod list_meetings;
mod search_knowledge;
#[cfg(test)]
pub(crate) mod tests;

use axum::Router;
use axum::routing::get;

pub use self::get_document::{DocumentArguments, open};
pub use self::list_meetings::{ListMeetingsRequest, list as list_meetings};
pub use self::search_knowledge::{SearchKnowledgeRequest, search};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/search", get(search_knowledge::handle))
        .route("/documents/{id}", get(get_document::handle))
        .route("/meetings", get(list_meetings::handle))
}
