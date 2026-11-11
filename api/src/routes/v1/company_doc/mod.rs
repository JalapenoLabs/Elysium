// Copyright © 2026 Jalapeno Labs

//! `/api/v1/company-doc`: starting the company doc's review now. See `docs/knowledge.md`, The
//! company doc.
//!
//! Everything else about the doc is its sections' pages': people read, edit, and restore them, and
//! accept or dismiss what agents suggested to them, through `/api/v1/pages`. The route answers the
//! company's members alone ([`crate::auth::MemberUser`]): the doc is part of the knowledge base,
//! which a guest never reaches.

mod start_review;

use axum::Router;
use axum::routing::post;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/review", post(start_review::handle))
}
