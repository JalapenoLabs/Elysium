// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/agent-guidance/versions/{version}/restore`: an earlier version's text made current
//! again, as the next version, which names the one it copied. Nothing is lost: the versions in
//! between stay, and restoring one of them puts it back in turn.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};

use crate::agent_guidance::AgentGuidanceResponse;
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    path: Result<Path<i32>, PathRejection>,
) -> Result<Json<AgentGuidanceResponse>, ApiError> {
    let Path(version) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let restored = state
        .agent_guidance
        .restore(&mut connection, version, admin.id())
        .await?;
    if restored.is_some() {
        state.events.publish(&ServerEvent::AgentGuidanceChanged);
    }

    Ok(Json(state.agent_guidance.read(&mut connection).await?))
}
