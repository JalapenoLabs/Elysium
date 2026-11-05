// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/pages/{id}/publish`: publish a page's next version, the one write of its content.
//!
//! A publish names the version it was edited from, and a later one is refused with
//! `409 page_changed`, so nobody overwrites a version they have not seen. Text the page already
//! holds makes no version and answers `outcome: "unchanged"`.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use schemars::JsonSchema;
use serde::Deserialize;
use uuid::Uuid;

use super::{PublishedPage, answer_published, author, learning_fields, page_edit};
use crate::auth::MemberUser;
use crate::database::Pool;
use crate::errors::ApiError;
use crate::models::agent::Author;
use crate::models::page;
use crate::realtime::EventBus;
use crate::state::AppState;

/// A page's next version. Shaped and bounded as creating a page is.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishPageRequest {
    /// The version this was edited from, the page's `currentVersion` when it was read. Publishing
    /// over a newer one is refused, so read the page again and make the change on what it says now.
    pub base_version: i32,
    /// Its title, on one line of up to 300 characters.
    pub title: String,
    /// What it says, in Markdown, up to 100,000 characters: the page whole, not a change to it. A
    /// learning's is up to 20,000 and never empty, and a company doc section's up to 12,000.
    pub body: String,
    /// The key of the category a learning is filed under, given with its evidence. Left out, a
    /// learning keeps its own; a page that is no learning has neither.
    #[serde(default)]
    pub category: Option<String>,
    /// What shows a learning is true, up to 20,000 characters, given with its category.
    #[serde(default)]
    pub evidence: Option<String>,
}

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<PublishPageRequest>, JsonRejection>,
) -> Result<Json<PublishedPage>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let published = publish(&state.database, &state.events, author(&member), id, body).await?;
    Ok(Json(published))
}

/// Publishes `request` over page `id` as `author`: what this route and an agent's tool both do.
///
/// # Errors
/// The request's validation errors, `404` for an unknown page, `400` for an unknown category or a
/// body a learning or section cannot hold, and `409` with `page_changed`, `page_deleted`,
/// `page_proposal_only`, `page_fixed`, `page_title_taken`, or `page_not_a_learning` for a page that
/// refuses it.
pub async fn publish(
    database: &Pool,
    events: &EventBus,
    author: Author<'_>,
    id: Uuid,
    request: PublishPageRequest,
) -> Result<PublishedPage, ApiError> {
    let mut edit = page_edit(&request.title, &request.body)?;
    edit.learning = learning_fields(request.category.as_deref(), request.evidence.as_deref())?;
    let mut connection = database
        .get()
        .await
        .context("no database connection available")?;
    let recorded = page::publish(&mut connection, id, edit, request.base_version, author).await?;
    answer_published(events, &mut connection, recorded).await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_publish_names_the_version_it_was_made_from() {
        let request: PublishPageRequest = serde_json::from_value(json!({
            "baseVersion": 3,
            "title": "Runbooks",
            "body": "Restart it.",
        }))
        .expect("parses");
        assert_eq!(request.base_version, 3);

        serde_json::from_value::<PublishPageRequest>(json!({
            "title": "Runbooks",
            "body": "Restart it.",
        }))
        .expect_err("the base version is required");
    }

    #[test]
    fn a_learnings_category_and_evidence_may_be_published_with_it() {
        let request: PublishPageRequest = serde_json::from_value(json!({
            "baseVersion": 3,
            "title": "Lint first",
            "body": "Run the linter before the build.",
            "category": "convention",
            "evidence": "It caught it on WAY-123.",
        }))
        .expect("parses");
        assert_eq!(request.category.as_deref(), Some("convention"));
        assert_eq!(
            request.evidence.as_deref(),
            Some("It caught it on WAY-123.")
        );
    }
}
