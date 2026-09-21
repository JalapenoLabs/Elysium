// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/jira-credentials/{id}/filters`: the saved filters the stored token can see,
//! for linking one to an initiative as a container.
//!
//! A filter is not bounded by the allowlist, since it belongs to no project; its children
//! are, because the watcher reads them through a search bounded like every other.

use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::open;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let stored = open(&state, id).await?;
    let listing = state.jira.list_filters(&stored.site()).await?;

    Ok(Json(json!({
        "filters": listing.items,
        "truncated": listing.truncated,
    })))
}
