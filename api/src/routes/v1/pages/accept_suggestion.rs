// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/pages/{id}/suggestions/{suggestionId}/accept`: write a suggested edit into its
//! page, wholly or in part, as the member accepting it.
//!
//! The chosen changes are merged with what changed on the page since the suggestion was written; a
//! change whose lines were changed since answers `409 suggestion_overlaps` and the suggestion stays
//! pending. Answers the suggestion as decided and the page as it now reads.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{answer_decided, author};
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::page_suggestion;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// The changes to take, by their `index`; left out to take every one.
    #[serde(default)]
    changes: Option<Vec<usize>>,
}

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((page_id, suggestion_id)) = path?;
    let Json(body) = body?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let decided = page_suggestion::accept(
        &mut connection,
        page_id,
        suggestion_id,
        body.changes,
        author(&member),
    )
    .await?;
    let (suggestion, page) = answer_decided(&state.events, &mut connection, decided).await?;
    Ok(Json(json!({ "suggestion": suggestion, "page": page })))
}
