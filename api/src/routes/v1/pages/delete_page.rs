// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/pages/{id}`: delete a page softly, with every live page under it. They leave the
//! knowledge index at once and keep their versions, and restoring the page brings them back.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use super::{answer, author};
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::agent::Author;
use crate::models::page;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    delete(&state, author(&member), id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Deletes page `id` as `author`: what this route and an agent's tool both do. Deleting a deleted
/// page changes nothing.
///
/// # Errors
/// `404` for an unknown page, and `409 page_proposal_only` when an agent's deletion would take a
/// page agents only propose to.
pub async fn delete(state: &AppState, author: Author<'_>, id: Uuid) -> Result<(), ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let recorded = page::delete(&mut connection, id, author).await?;
    answer(state, &mut connection, recorded).await?;
    Ok(())
}
