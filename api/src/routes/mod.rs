// Copyright © 2026 Jalapeno Labs

//! HTTP routes. Every public path is mounted under `/api`, matching nginx's proxy prefix.
//! Health and build routes sit at the top level; resources are versioned under `/api/v1`.
//! `/internal` answers Kratos alone, on the compose network.

mod internal;
mod ok;
mod ping;
pub mod v1;
mod version;

use axum::Router;
use axum::routing::get;

use crate::state::AppState;

/// Builds the `/api` router, and `/internal` for Kratos.
pub fn router(state: &AppState) -> Router<AppState> {
    let api = Router::new()
        .route("/ok", get(ok::handle))
        .route("/ping", get(ping::handle))
        .route("/version", get(version::handle))
        .nest("/v1", v1::router(state));

    Router::new()
        .nest("/api", api)
        .nest("/internal", internal::router(state))
}
