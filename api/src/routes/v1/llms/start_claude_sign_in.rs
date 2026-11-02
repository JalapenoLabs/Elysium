// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/llms/claude-sign-ins`: start signing a Claude account in through the CLI.
//!
//! Answers the sign-in page to open. Anthropic's page then shows a code, which the person
//! pastes into `POST /api/v1/llms/claude-sign-ins/{id}`.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde_json::{Value, json};

use super::sign_in_error;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let started = state
        .claude_accounts
        .sign_ins()
        .start(current.id())
        .await
        .map_err(sign_in_error)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "signIn": {
                "id": started.id,
                "authorizeUrl": started.authorize_url,
                "expiresAt": started.expires_at,
            }
        })),
    ))
}
