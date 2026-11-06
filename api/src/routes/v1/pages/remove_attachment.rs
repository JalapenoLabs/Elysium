// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/pages/{id}/attachments/{attachmentId}`: take a file off a page. Its address stops
//! answering, so an image in the page's text says it is unavailable, and it can be put back
//! (`restore_attachment`). The file stays with the provider until a page is deleted for good, which
//! does not exist yet (`crate::models::page_attachment`).

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
use crate::models::page_attachment;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, attachment_id)) = path?;
    let page = remove(&state, author(&member), id, attachment_id).await?;
    Ok(Json(json!({ "page": page })))
}

/// Takes `attachment_id` off page `id` as `author`: what this route does, and what an agent's tool
/// would.
///
/// # Errors
/// `404` for an unknown page or attachment, and `409` with `page_deleted` or `page_proposal_only` for
/// a page that refuses it.
pub async fn remove(
    state: &AppState,
    author: Author<'_>,
    id: Uuid,
    attachment_id: Uuid,
) -> Result<PageResponse, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let recorded = page_attachment::remove(&mut connection, id, attachment_id, author).await?;
    answer(state, &mut connection, recorded).await
}
