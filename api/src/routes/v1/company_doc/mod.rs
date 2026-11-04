// Copyright © 2026 Jalapeno Labs

//! `/api/v1/company-doc`: the one document that knows the company, its pending proposals, and
//! its history. See `docs/knowledge.md`, The company doc.
//!
//! Every route answers the company's members alone ([`MemberUser`]): the doc is part of the
//! knowledge base, which a guest never reaches. Any member edits it, approves or rejects what
//! was proposed, undoes any change, and starts a review. Proposing is for everyone else, the
//! review and agents, through `crate::company_doc::propose`; a person edits directly.

mod approve_proposal;
mod edit_doc;
mod get_doc;
mod list_history;
mod reject_proposal;
mod start_review;
#[cfg(test)]
mod tests;
mod undo_change;

use axum::Router;
use axum::routing::{get, post};
use chrono::Utc;

use crate::auth::MemberUser;
use crate::models::company_doc::Author;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_doc::handle).patch(edit_doc::handle))
        .route("/history", get(list_history::handle))
        .route("/history/{entry_id}/undo", post(undo_change::handle))
        .route("/proposals/{id}/approve", post(approve_proposal::handle))
        .route("/proposals/{id}/reject", post(reject_proposal::handle))
        .route("/review", post(start_review::handle))
}

/// Who a change through these routes is by: the member, now.
fn author(member: &MemberUser) -> Author {
    Author {
        user_id: member.id(),
        at: Utc::now(),
    }
}
