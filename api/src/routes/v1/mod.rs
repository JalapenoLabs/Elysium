// Copyright © 2026 Jalapeno Labs

//! Version 1 resource routes, mounted at `/api/v1`.

mod llms;

use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().nest("/llms", llms::router())
}
