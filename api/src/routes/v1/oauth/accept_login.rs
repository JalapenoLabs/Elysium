// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/oauth/login/accept`: tell Hydra who is signing in for an MCP client: the person
//! Kratos says is signed in here. Answers where the browser goes next.

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use serde_json::{Value, json};

use super::ChallengeBody;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    body: Result<Json<ChallengeBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body?;
    let hydra = &state.auth.hydra;

    // Looked up first, so an unknown or used challenge answers 404 rather than a Hydra error.
    hydra.login_request(&body.challenge).await?;
    let redirect_to = hydra.accept_login(&body.challenge, current.id()).await?;
    Ok(Json(json!({ "redirectTo": redirect_to })))
}
