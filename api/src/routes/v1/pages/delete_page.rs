// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/pages/{id}`: delete a page softly, with every live page under it. They leave the
//! knowledge index at once and keep their versions, and restoring the page brings them back. With
//! `?keepChildren=true` the page goes alone: its children take its place in the tree, each with
//! what is under it, and restoring it moves them back.

use anyhow::Context;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use uuid::Uuid;

use super::{answer, author};
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::agent::Author;
use crate::models::page;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteQuery {
    #[serde(default)]
    keep_children: bool,
}

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
    query: Result<Query<DeleteQuery>, QueryRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    let Query(query) = query?;
    delete(&state, author(&member), id, query.keep_children).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Deletes page `id` as `author`, with everything under it, or alone with `keep_children`: what this
/// route and an agent's tool both do. Deleting a deleted page changes nothing.
///
/// # Errors
/// `404` for an unknown page, `409 page_fixed` for a page Elysium relies on, and
/// `409 page_proposal_only` when an agent's deletion would take or move a page agents only propose
/// to.
pub async fn delete(
    state: &AppState,
    author: Author<'_>,
    id: Uuid,
    keep_children: bool,
) -> Result<(), ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let recorded = if keep_children {
        page::delete_keeping_children(&mut connection, id, author).await?
    } else {
        page::delete(&mut connection, id, author, None).await?
    };
    answer(state, &mut connection, recorded).await?;
    Ok(())
}
