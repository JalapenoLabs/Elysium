// Copyright © 2026 Jalapeno Labs

//! `/api/v1/backups`: where the databases are backed up, every attempt, and Back up now, for
//! admins. See `crate::backups`.

mod choose_destination;
mod get_backups;
mod start_backup;

use axum::Router;
use axum::routing::{get, put};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_backups::handle).post(start_backup::handle))
        .route("/destination", put(choose_destination::handle))
}
