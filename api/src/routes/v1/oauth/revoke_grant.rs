// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/oauth/grants/{client_id}`: disconnect one MCP client from the signed-in person.
//! Every token issued to it for them is refused from its next request on.

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::oauth::tokens;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(client_id) = path?;
    tokens::revoke_grant(&state.redis, &state.auth.hydra, current.id(), &client_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
