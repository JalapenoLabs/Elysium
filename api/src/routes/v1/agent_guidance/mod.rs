// Copyright © 2026 Jalapeno Labs

//! `/api/v1/agent-guidance`: the guidance every connected agent reads as its session starts, and
//! every version of it. See `docs/knowledge.md`, Agent guidance.
//!
//! The company's members read it, so anyone can copy its `CLAUDE.md` block into a repository; a
//! guest reaches no knowledge base. Only admins save a version or restore an earlier one, since
//! what they write reaches every agent. Every write answers the guidance whole, as the read does,
//! and announces `agentGuidance.changed`.

mod get_guidance;
mod restore_version;
mod save_guidance;
#[cfg(test)]
mod tests;

use axum::Router;
use axum::routing::{get, post};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_guidance::handle).put(save_guidance::handle))
        .route("/versions/{version}/restore", post(restore_version::handle))
}
