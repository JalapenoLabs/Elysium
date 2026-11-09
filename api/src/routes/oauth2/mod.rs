// Copyright © 2026 Jalapeno Labs

//! `/oauth2/register`: MCP clients registering themselves, and managing their registrations,
//! relayed to Hydra's public API (`crate::oauth::registration`). See `docs/mcp.md`.
//!
//! These are the paths Hydra's metadata advertises, on Elysium's origin, so nothing a client reads
//! changes. nginx sends them here and every other `/oauth2/` path to Hydra itself. Registering is
//! open to anyone, as MCP clients register themselves: a client reaches nothing until an approved
//! person consents to it, and nginx throttles these paths per client address. Managing a
//! registration needs its registration access token, which Hydra checks.

mod manage_registration;
mod register;
#[cfg(test)]
mod tests;

use axum::Router;
use axum::routing::{any, get, post};

use super::not_found;
use crate::oauth::registration::Registration;

/// The registration routes under `/oauth2/register`, merged into the root router as they are, with
/// the relay as their state rather than the application's, so the relay is all they can reach.
pub fn router<State>(registration: Registration) -> Router<State>
where
    State: Clone + Send + Sync + 'static,
{
    let registration_routes = Router::new()
        .route("/", post(register::handle))
        .route(
            "/{client_id}",
            get(manage_registration::handle)
                .put(manage_registration::handle)
                .delete(manage_registration::handle),
        )
        // Its own fallback, as `/api` has: nginx sends every path under `/oauth2/register` here, and
        // one that matches nothing is a JSON 404 rather than the web app the root falls back to.
        .fallback(not_found)
        .with_state(registration);
    Router::new()
        .nest("/oauth2/register", registration_routes)
        // A bare trailing slash is outside the nest, which routes it nowhere, so it is refused here.
        .route("/oauth2/register/", any(not_found))
}
