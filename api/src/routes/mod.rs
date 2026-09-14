// Copyright © 2026 Jalapeno Labs

//! HTTP routes. Every path is mounted under `/api`, matching nginx's proxy prefix.
//! Health and build routes sit at the top level; resources are versioned under `/api/v1`.

mod ok;
mod ping;
pub mod v1;
mod version;

use axum::Router;
use axum::routing::get;

use crate::state::AppState;

/// Builds the `/api` router.
pub fn router() -> Router<AppState> {
    let api = Router::new()
        .route("/ok", get(ok::handle))
        .route("/ping", get(ping::handle))
        .route("/version", get(version::handle))
        .nest("/v1", v1::router());

    Router::new().nest("/api", api)
}
