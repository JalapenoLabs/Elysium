// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/pages/{id}/labels/{label}`: take a label off a page. Like adding one, it makes no
//! version, and a label the page does not carry records nothing.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
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

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<(Uuid, String)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, label)) = path?;
    let page = remove(&state.database, &state.events, author(&member), id, &label).await?;
    Ok(Json(json!({ "page": page })))
}

/// Takes `label` off page `id` as `author`: what this route and an agent's tool both do.
///
/// # Errors
/// `422` for a label that is not well formed, `404` for an unknown page, and `409 page_deleted` for
/// a deleted one.
pub async fn remove(
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
    let recorded = page_label::remove(&mut connection, id, &label, author).await?;
    answer(events, &mut connection, recorded).await
}
