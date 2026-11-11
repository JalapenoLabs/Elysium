// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/pages/{id}/suggestions/{suggestionId}/dismiss`: turn a suggested edit down, saying
//! why. The page is left as it is; the suggestion keeps who dismissed it, when, and why, which the
//! company doc's review reads so it does not suggest the same edit again. Answers the suggestion as
//! decided and the page.

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
    /// Why, in up to 2,000 characters, trimmed.
    reason: String,
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
    let decided = page_suggestion::dismiss(
        &mut connection,
        page_id,
        suggestion_id,
        &body.reason,
        author(&member),
    )
    .await?;
    let (suggestion, page) = answer_decided(&state.events, &mut connection, decided).await?;
    Ok(Json(json!({ "suggestion": suggestion, "page": page })))
}
