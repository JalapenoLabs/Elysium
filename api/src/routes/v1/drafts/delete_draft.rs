// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/drafts/{id}`: throw a draft away; `409` while it is being sent. A sent
//! draft's record stays on the timeline.

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::drafts;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    drafts::discard(&state, &current.0, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
