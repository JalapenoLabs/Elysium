// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/pages/{id}/labels`: put a label on a page. It makes no version: the page's history
//! records it, and its document in the knowledge index reads it at once. A label already there
//! records nothing.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{PageResponse, answer, author};
use crate::auth::MemberUser;
use crate::database::Pool;
use crate::errors::ApiError;
use crate::models::agent::Author;
use crate::models::page_label::{self, Label};
use crate::realtime::EventBus;
use crate::state::AppState;

/// The label to put on a page.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddLabelRequest {
    /// Up to 50 characters: letters, digits, hyphens, underscores, and dots, starting with a letter
    /// or digit. Stored lowercase with spaces as hyphens, so `On Call` is the label `on-call`.
    pub label: String,
}

/// A page and a label: what the label tools take, where a route has the page in its path.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageLabelArguments {
    /// The page, from `page_list`.
    pub id: Uuid,
    /// Up to 50 characters: letters, digits, hyphens, underscores, and dots, starting with a letter
    /// or digit. Stored lowercase with spaces as hyphens, so `On Call` is the label `on-call`.
    pub label: String,
}

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<AddLabelRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let page = add(
        &state.database,
        &state.events,
        author(&member),
        id,
        &body.label,
    )
    .await?;
    Ok(Json(json!({ "page": page })))
}

/// Puts `label` on page `id` as `author`: what this route and an agent's tool both do. Any
/// person or agent may label any live page, a proposal-only or fixed one included, since a label
/// changes neither what it says nor where it sits.
///
/// # Errors
/// `422` for a label that is not well formed, `404` for an unknown page, and `409` with
/// `page_deleted` or `page_label_limit` for one that is refused.
pub async fn add(
    database: &Pool,
    events: &EventBus,
    author: Author<'_>,
    id: Uuid,
    label: &str,
) -> Result<PageResponse, ApiError> {
    let label = Label::parse(label)?;
    let mut connection = database
        .get()
        .await
        .context("no database connection available")?;
    let recorded = page_label::add(&mut connection, id, &label, author).await?;
    answer(events, &mut connection, recorded).await
}
