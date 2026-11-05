// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/pages/{id}/versions/{version}/restore`: an earlier version's content published as
//! the next version, which names the one it copied. Nothing is lost: the versions in between stay,
//! and restoring one of them puts it back in turn. A version whose content the page already holds
//! makes no version and answers `outcome: "unchanged"`.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use uuid::Uuid;

use super::{PublishedPage, answer_published, author};
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::agent::Author;
use crate::models::page;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<(Uuid, i32)>, PathRejection>,
) -> Result<Json<PublishedPage>, ApiError> {
    let Path((id, version)) = path?;
    let restored = restore_version(&state, author(&member), id, version).await?;
    Ok(Json(restored))
}

/// Restores version `version` of page `id` as `author`: what this route and an agent's tool both
/// do.
///
/// # Errors
/// `404` for an unknown page or version, and `409` with `page_deleted` or `page_proposal_only` for
/// a page that refuses it.
pub async fn restore_version(
    state: &AppState,
    author: Author<'_>,
    id: Uuid,
    version: i32,
) -> Result<PublishedPage, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let recorded = page::restore_version(&mut connection, id, version, author).await?;
    answer_published(&state.events, &mut connection, recorded).await
}
