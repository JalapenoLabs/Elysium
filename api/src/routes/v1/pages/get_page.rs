// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/pages/{id}`: one page whole, live or deleted, with who published it last.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{PageResponse, SuggestionResponse, pending_suggestions, response};
use crate::auth::MemberUser;
use crate::database::Pool;
use crate::errors::ApiError;
use crate::models::page;
use crate::state::AppState;

/// Names one page, which a route takes from its path.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageArguments {
    /// The page's id, from `page_list`, or the `pageId` of a citation that names one.
    pub id: Uuid,
}

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let page = read(&state.database, id).await?;
    Ok(Json(json!({ "page": page })))
}

/// Page `id`: what this route and an agent's tool both read.
///
/// # Errors
/// `404` for an unknown page.
pub async fn read(database: &Pool, id: Uuid) -> Result<PageResponse, ApiError> {
    let mut connection = database
        .get()
        .await
        .context("no database connection available")?;
    let page = page::find(&mut connection, id).await?;
    response(&mut connection, page).await
}

/// The most suggestions `page_read` answers beside a page, the oldest first. Claude Code refuses a
/// tool's result past about 25,000 tokens, and each suggestion carries its changes with their lines
/// around them; the page's `pendingSuggestions` says how many wait in all, and the same edit is
/// pending once, so an agent rarely meets the bound.
const READ_SUGGESTIONS_MAX: usize = 20;

/// Page `id` as an agent's `page_read` reads it: the page whole and the oldest
/// [`READ_SUGGESTIONS_MAX`] suggestions waiting on it, so an agent knows what a person may still
/// accept and need not suggest it again.
///
/// # Errors
/// `404` for an unknown page.
pub async fn read_with_suggestions(
    database: &Pool,
    id: Uuid,
) -> Result<(PageResponse, Vec<SuggestionResponse>), ApiError> {
    let mut connection = database
        .get()
        .await
        .context("no database connection available")?;
    let page = page::find(&mut connection, id).await?;
    let page = response(&mut connection, page).await?;
    let mut suggestions = pending_suggestions(&mut connection, id).await?;
    suggestions.truncate(READ_SUGGESTIONS_MAX);
    Ok((page, suggestions))
}
