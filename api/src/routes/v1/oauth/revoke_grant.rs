// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/oauth/grants/{client_id}`: disconnect one MCP client from the signed-in person.
//! Every token issued to it for them stops working.

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(client_id) = path?;
    state
        .auth
        .hydra
        .revoke_consent(current.id(), Some(&client_id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
