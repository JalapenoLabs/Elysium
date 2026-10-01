// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/oauth/consent/reject`: decline to connect an MCP client. Answers where the browser
//! goes next: back to the client, told access was denied, or that it asked for nothing Elysium
//! can grant.

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use serde_json::{Value, json};

use super::{ChallengeBody, grantable, own_consent};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::oauth::hydra::ConsentRefusal;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    body: Result<Json<ChallengeBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body?;
    let request = own_consent(&state, &current, &body.challenge).await?;
    // Asking for nothing grantable is the client's mistake to fix, not the person's refusal.
    let refusal = if grantable(&request.requested_scope).is_empty() {
        ConsentRefusal::NothingGrantable
    } else {
        ConsentRefusal::Denied
    };
    let redirect_to = state
        .auth
        .hydra
        .reject_consent(&body.challenge, refusal)
        .await?;
    Ok(Json(json!({ "redirectTo": redirect_to })))
}
