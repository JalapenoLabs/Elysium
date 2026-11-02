// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/connectors/activity`: the newest syncs worth showing across every connector the
//! person sees, newest first: the ones that recorded something, and the ones that failed.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};
use uuid::Uuid;

use super::RunView;
use crate::auth::CurrentUser;
use crate::connectors::kinds::workspace_kinds;
use crate::errors::ApiError;
use crate::models::{connector, connector_run};
use crate::state::AppState;

/// How many syncs the feed shows.
const FEED_LENGTH: i64 = 50;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let ids: Vec<Uuid> = connector::list_visible(&mut connection, current.id(), &workspace_kinds())
        .await?
        .into_iter()
        .map(|visible| visible.id)
        .collect();
    let runs: Vec<RunView> = connector_run::recent(&mut connection, &ids, FEED_LENGTH)
        .await?
        .into_iter()
        .map(RunView::from)
        .collect();
    Ok(Json(json!({ "runs": runs })))
}
