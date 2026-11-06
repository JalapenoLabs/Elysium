// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/pages/{id}/attachments/{attachmentId}/restore`: put a removed file back on its page,
//! at the address it had, so whatever in the page's text refers to it shows it again.

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
    let page = restore(&state, author(&member), id, attachment_id).await?;
    Ok(Json(json!({ "page": page })))
}

/// Puts `attachment_id` back on page `id` as `author`: what this route does, and what an agent's tool
/// would.
///
/// # Errors
/// `404` for an unknown page or attachment, and `409` with `page_deleted`, `page_proposal_only`, or
/// `page_attachment_limit` for a page that refuses it.
pub async fn restore(
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
    let recorded = page_attachment::restore(&mut connection, id, attachment_id, author).await?;
    answer(state, &mut connection, recorded).await
}
