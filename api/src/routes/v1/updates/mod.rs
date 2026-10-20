// Copyright © 2026 Jalapeno Labs

//! `/api/v1/updates`: the release running, auto-update's state, and the update history,
//! for admins. See `crate::updates`.

mod check_updates;
mod get_updates;
mod request_update;

use axum::Router;
use axum::routing::{get, post};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_updates::handle))
        .route("/check", post(check_updates::handle))
        .route("/apply", post(request_update::handle))
}
