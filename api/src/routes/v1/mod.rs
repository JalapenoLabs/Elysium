// Copyright © 2026 Jalapeno Labs

//! Version 1 resource routes, mounted at `/api/v1`.

pub mod action_items;
mod auth_status;
mod authenticator_nickname;
mod authenticator_replacement;
mod backups;
pub mod browser_sessions;
pub mod changesets;
pub mod coding_sessions;
pub mod company_doc;
pub mod connectors;
pub mod drafts;
pub mod elysia;
pub mod environment_variables;
mod events;
pub mod github_credentials;
pub mod initiatives;
pub mod issue_reports;
pub mod jira_credentials;
pub mod knowledge;
pub mod learning_categories;
pub mod learnings;
pub mod llms;
pub mod mail;
mod me;
pub mod meetings;
mod model_settings;
mod my_sessions;
pub mod oauth;
pub mod password_vault;
pub mod projects;
mod runs;
mod search;
mod sign_out;
pub mod storage_locations;
pub mod studio_items;
pub mod timeline;
mod updates;
pub mod users;
pub mod workspace_settings;

use axum::Router;
use axum::middleware::from_fn_with_state;
use axum::routing::{delete, get, post, put};

use crate::auth::middleware::{authenticate, require_access, require_same_origin};
use crate::state::AppState;

/// Every route but `/auth/status` and `/auth/sign-out` needs a session; every route but those
/// and `/me` needs an active person with whatever the workspace requires of them. See
/// `crate::auth`.
pub fn router(state: &AppState) -> Router<AppState> {
    let workspace =
        workspace_router().route_layer(from_fn_with_state(state.clone(), require_access));
    let signed_in = Router::new()
        .route("/me", get(me::handle))
        .route(
            "/me/authenticator-nickname",
            put(authenticator_nickname::set).delete(authenticator_nickname::clear),
        )
        .route(
            "/me/authenticator-replacement",
            post(authenticator_replacement::start),
        )
        .route(
            "/me/authenticator-replacement/confirm",
            post(authenticator_replacement::confirm),
        )
        .route("/me/sessions", delete(my_sessions::end_others))
        .route("/me/sessions/{id}", delete(my_sessions::end_one))
        .merge(workspace)
        .route_layer(from_fn_with_state(state.clone(), authenticate));

    Router::new()
        // Signing out needs no session of its own: one waiting on its second factor signs out too.
        .route("/auth/sign-out", post(sign_out::handle))
        .merge(signed_in)
        .route_layer(from_fn_with_state(state.clone(), require_same_origin))
        .route("/auth/status", get(auth_status::handle))
}

fn workspace_router() -> Router<AppState> {
    Router::new()
        .route("/events", get(events::handle))
        .nest("/backups", backups::router())
        .nest("/action-items", action_items::router())
        .nest("/browser-sessions", browser_sessions::router())
        .nest("/changesets", changesets::router())
        .nest("/company-doc", company_doc::router())
        .nest("/connectors", connectors::router())
        .nest("/drafts", drafts::router())
        .nest("/elysia", elysia::router())
        .nest("/environment-variables", environment_variables::router())
        .nest("/github-credentials", github_credentials::router())
        .nest("/initiatives", initiatives::router())
        .nest("/issue-reports", issue_reports::router())
        .nest("/jira-credentials", jira_credentials::router())
        .nest("/knowledge", knowledge::router())
        .nest("/learning-categories", learning_categories::router())
        .nest("/learnings", learnings::router())
        .nest("/llms", llms::router())
        .nest("/mail", mail::router())
        .nest("/meetings", meetings::router())
        .nest("/model-settings", model_settings::router())
        .nest("/projects", projects::router())
        .nest("/runs", runs::router())
        .nest("/search", search::router())
        .nest("/storage-locations", storage_locations::router())
        .nest("/timeline", timeline::router())
        .nest("/studio-items", studio_items::router())
        .nest("/coding-sessions", coding_sessions::router())
        .nest("/oauth", oauth::router())
        .nest("/password-vault", password_vault::router())
        .nest("/updates", updates::router())
        .nest("/users", users::router())
        .nest("/workspace-settings", workspace_settings::router())
}
