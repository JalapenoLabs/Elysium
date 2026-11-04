// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/agent-guidance`: the current guidance, every version newest first, the rules
//! Elysium adds to it, its `CLAUDE.md` block, and the longest guidance this deployment takes.

use anyhow::Context;
use axum::Json;
use axum::extract::State;

use crate::agent_guidance::AgentGuidanceResponse;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
) -> Result<Json<AgentGuidanceResponse>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    Ok(Json(state.agent_guidance.read(&mut connection).await?))
}
