// Copyright © 2026 Jalapeno Labs

//! Version 1 resource routes, mounted at `/api/v1`.

pub mod action_items;
mod auth_status;
pub mod changesets;
pub mod coding_sessions;
pub mod environment_variables;
mod events;
pub mod github_credentials;
pub mod initiatives;
pub mod jira_credentials;
pub mod llms;
pub mod mail;
mod me;
pub mod oauth;
pub mod projects;
pub mod satellites;
pub mod storage_locations;
pub mod studio_items;
pub mod users;
pub mod workspace_settings;

use axum::Router;
use axum::middleware::from_fn_with_state;
use axum::routing::get;

use crate::auth::middleware::{authenticate, require_access, require_same_origin};
use crate::state::AppState;

/// Every route but `/auth/status` needs a session; every route but that and `/me` needs an
/// active person with whatever the workspace requires of them. See `crate::auth`.
pub fn router(state: &AppState) -> Router<AppState> {
    let workspace =
        workspace_router().route_layer(from_fn_with_state(state.clone(), require_access));
    let signed_in = Router::new()
        .route("/me", get(me::handle))
        .merge(workspace)
        .route_layer(from_fn_with_state(state.clone(), authenticate))
        .route_layer(from_fn_with_state(state.clone(), require_same_origin));

    Router::new()
        .route("/auth/status", get(auth_status::handle))
        .merge(signed_in)
}

fn workspace_router() -> Router<AppState> {
    Router::new()
        .route("/events", get(events::handle))
        .nest("/action-items", action_items::router())
        .nest("/changesets", changesets::router())
        .nest("/environment-variables", environment_variables::router())
        .nest("/github-credentials", github_credentials::router())
        .nest("/initiatives", initiatives::router())
        .nest("/jira-credentials", jira_credentials::router())
        .nest("/llms", llms::router())
        .nest("/mail", mail::router())
        .nest("/projects", projects::router())
        .nest("/satellites", satellites::router())
        .nest("/storage-locations", storage_locations::router())
        .nest("/studio-items", studio_items::router())
        .nest("/coding-sessions", coding_sessions::router())
        .nest("/oauth", oauth::router())
        .nest("/users", users::router())
        .nest("/workspace-settings", workspace_settings::router())
}
