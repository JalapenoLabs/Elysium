// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/pages/{id}/restore`: bring a deleted page back where it was, with the pages its
//! deletion took. Restoring a live page changes nothing.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{PageResponse, answer, author};
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::agent::Author;
use crate::models::page;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let page = restore(&state, author(&member), id).await?;
    Ok(Json(json!({ "page": page })))
}

/// Restores page `id` as `author`: what this route and an agent's tool both do.
///
/// # Errors
/// `404` for an unknown page, `409 page_parent_gone` while its parent is deleted, and
/// `409 page_proposal_only` when an agent's restore would bring back a page agents only propose to.
pub async fn restore(
    state: &AppState,
    author: Author<'_>,
    id: Uuid,
) -> Result<PageResponse, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let recorded = page::restore(&mut connection, id, author, None).await?;
    answer(&state.events, &mut connection, recorded).await
}
