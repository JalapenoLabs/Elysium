// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/pages`: every live page without its text, in order among its siblings, which a
//! client builds the tree from by `parentId`; or with `?deleted=true` every deleted one, most
//! recently deleted first.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Value, json};

use super::PageSummaryResponse;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::{page, page_label};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListQuery {
    #[serde(default)]
    deleted: bool,
}

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query?;
    let pages = list(&state, query.deleted).await?;
    Ok(Json(json!({ "pages": pages })))
}

/// The tree's live pages, or the deleted ones: what this route and an agent's tool both read.
///
/// # Errors
/// Any database error.
pub async fn list(state: &AppState, deleted: bool) -> Result<Vec<PageSummaryResponse>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let mut labels = page_label::by_page(&mut connection).await?;
    let pages = page::list(&mut connection, deleted)
        .await?
        .into_iter()
        .map(|page| {
            let labels = labels.remove(&page.id).unwrap_or_default();
            PageSummaryResponse::new(page, labels)
        })
        .collect();
    Ok(pages)
}
