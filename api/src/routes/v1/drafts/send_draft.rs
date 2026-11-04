// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/drafts/{id}/send`: send a draft as the signed-in person, and answer it as the
//! send left it. A send that failed answers `200` with the draft `failed` and the reason in
//! `lastError`, its text kept, since the failure is the provider's answer rather than a bad
//! request. `409` while another send of it is under way or once it was sent.

use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::DraftResponse;
use crate::auth::CurrentUser;
use crate::drafts;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let sent = drafts::send(&state, &current.0, id).await?;
    Ok(Json(json!({ "draft": DraftResponse::from(sent) })))
}
