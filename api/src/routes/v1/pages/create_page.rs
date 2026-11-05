// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/pages`: create a page, last among its parent's children, as its version 1.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{PageResponse, answer, author, page_edit};
use crate::auth::MemberUser;
use crate::database::Pool;
use crate::errors::ApiError;
use crate::models::agent::Author;
use crate::models::page;
use crate::realtime::EventBus;
use crate::state::AppState;

/// A page to create.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreatePageRequest {
    /// The page it goes under; left out, it goes at the top of the tree.
    #[serde(default)]
    pub parent_id: Option<Uuid>,
    /// Its title, on one line of up to 300 characters.
    pub title: String,
    /// What it says, in Markdown, up to 100,000 characters. A page may be empty and exist only to
    /// hold others.
    #[serde(default)]
    pub body: String,
}

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    body: Result<Json<CreatePageRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    let page = create(&state.database, &state.events, author(&member), body).await?;
    Ok((StatusCode::CREATED, Json(json!({ "page": page }))))
}

/// Creates a page from `request` as `author`: what this route and an agent's tool both do.
///
/// # Errors
/// The request's validation errors, and `409 page_parent_gone` for a parent that does not exist or
/// is deleted.
pub async fn create(
    database: &Pool,
    events: &EventBus,
    author: Author<'_>,
    request: CreatePageRequest,
) -> Result<PageResponse, ApiError> {
    let edit = page_edit(&request.title, &request.body)?;
    let mut connection = database
        .get()
        .await
        .context("no database connection available")?;
    let recorded = page::create(&mut connection, request.parent_id, edit, author).await?;
    answer(events, &mut connection, recorded).await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_page_needs_only_a_title() {
        let request: CreatePageRequest =
            serde_json::from_value(json!({ "title": "Runbooks" })).expect("parses");
        assert_eq!(request.parent_id, None);
        assert_eq!(request.body, "");

        serde_json::from_value::<CreatePageRequest>(json!({ "body": "No title." }))
            .expect_err("a title is required");
        serde_json::from_value::<CreatePageRequest>(json!({ "title": "Runbooks", "labels": [] }))
            .expect_err("unknown fields are refused");
    }
}
