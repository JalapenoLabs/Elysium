// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/pages/{id}/passages/{position}`: one passage of a page's document, as the search
//! cited it, so a reader opened at a passage can find it in the page's Markdown. The index cut the
//! text, so the web app reads the cut from it rather than repeating the cut. See
//! `docs/knowledge.md`, Pages, Finding pages.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::knowledge_document;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    path: Result<Path<(Uuid, i32)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, position)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let passage = knowledge_document::passage_of_page(&mut connection, id, position)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(json!({ "passage": passage })))
}
