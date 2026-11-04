// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/learnings/{id}`: delete a learning softly. It leaves the knowledge index at
//! once and keeps its history, so it can be restored, or the deletion undone, at any time.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use super::{announce, author};
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::learning::{self, Agent};
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    delete(&state, &member, None, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Deletes learning `id` as `member`, with `agent` acting for them: what both this route and the
/// MCP server's `learning_delete` do. Deleting a deleted learning changes nothing.
///
/// # Errors
/// `404` for an unknown learning.
pub async fn delete(
    state: &AppState,
    member: &MemberUser,
    agent: Option<&Agent>,
    id: Uuid,
) -> Result<(), ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let recorded = learning::delete(&mut connection, id, author(member, agent)).await?;
    announce(state, recorded);
    Ok(())
}
