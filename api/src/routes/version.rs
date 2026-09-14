// Copyright © 2026 Jalapeno Labs

//! `GET /api/version`: build identity and git history.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;

use crate::state::AppState;
use crate::version::VersionInfo;

/// Returns the [`VersionInfo`] captured at compile time.
pub async fn handle(State(state): State<AppState>) -> Json<Arc<VersionInfo>> {
    Json(Arc::clone(&state.version))
}
