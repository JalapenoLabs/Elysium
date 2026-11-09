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
use axum::routing::{get, post};

use crate::oauth::registration::Registration;

/// The registration routes at their full paths, merged into the root router as they are, with the
/// relay as their state rather than the application's, so the relay is all they can reach.
pub fn router<State>(registration: Registration) -> Router<State>
where
    State: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/oauth2/register", post(register::handle))
        .route(
            "/oauth2/register/{client_id}",
            get(manage_registration::handle)
                .put(manage_registration::handle)
                .delete(manage_registration::handle),
        )
        .with_state(registration)
}
