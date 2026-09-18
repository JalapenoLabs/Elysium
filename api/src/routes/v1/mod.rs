// Copyright © 2026 Jalapeno Labs

//! Version 1 resource routes, mounted at `/api/v1`.

pub mod action_items;
pub mod coding_sessions;
pub mod environment_variables;
mod events;
pub mod github_credentials;
pub mod initiatives;
pub mod jira_credentials;
pub mod llms;
pub mod mail;
pub mod projects;
pub mod satellites;
pub mod storage_locations;

use axum::Router;
use axum::routing::get;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/events", get(events::handle))
        .nest("/action-items", action_items::router())
        .nest("/environment-variables", environment_variables::router())
        .nest("/github-credentials", github_credentials::router())
        .nest("/initiatives", initiatives::router())
        .nest("/jira-credentials", jira_credentials::router())
        .nest("/llms", llms::router())
        .nest("/mail", mail::router())
        .nest("/projects", projects::router())
        .nest("/satellites", satellites::router())
        .nest("/storage-locations", storage_locations::router())
        .nest("/coding-sessions", coding_sessions::router())
}
