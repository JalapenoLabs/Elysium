// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/llms/claude-sign-ins/{id}`: abandon a sign-in, ending its CLI at once.

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use super::sign_in_error;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    state
        .claude_accounts
        .sign_ins()
        .cancel(id, current.id())
        .await
        .map_err(sign_in_error)?;
    Ok(StatusCode::NO_CONTENT)
}
