// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/drafts/{id}`: replace a draft's text. Where it goes stays as it was written;
//! `409` while it is being sent or once it was.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::DraftResponse;
use crate::auth::CurrentUser;
use crate::drafts;
use crate::errors::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    body: String,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let updated = drafts::update(&state, &current.0, id, &body.body).await?;
    Ok(Json(json!({ "draft": DraftResponse::from(updated) })))
}
