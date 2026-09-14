// Copyright © 2026 Jalapeno Labs

//! Version 1 resource routes, mounted at `/api/v1`.

pub mod coding_sessions;
mod events;
pub mod llms;
pub mod satellites;

use axum::Router;
use axum::routing::get;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/events", get(events::handle))
        .nest("/llms", llms::router())
        .nest("/satellites", satellites::router())
        .nest("/coding-sessions", coding_sessions::router())
}
