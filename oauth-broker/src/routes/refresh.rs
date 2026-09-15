// Copyright © 2026 Jalapeno Labs

//! `POST /v1/refresh`: an instance trades a refresh token for an access token.
//!
//! Providers require the client secret on every refresh, and only the broker holds
//! it, so every access token an instance uses passes through here. The broker still
//! stores nothing: the refresh token arrives in the request and leaves in the reply
//! when the provider rotates it.

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, AppState};
use crate::providers::{self, Provider, TokenError};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RefreshRequest {
    provider: Provider,
    refresh_token: SecretString,
}

pub(super) async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RefreshRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(request) = body.map_err(|rejection| ApiError::BadRequest {
        code: "invalid_request",
        message: rejection.body_text(),
    })?;

    let Some(credentials) = state.config.credentials(request.provider) else {
        return Err(ApiError::BadRequest {
            code: "provider_unavailable",
            message: "this broker does not offer that provider".to_owned(),
        });
    };

    let tokens = providers::refresh(
        &state.http,
        request.provider,
        credentials,
        &request.refresh_token,
    )
    .await
    .map_err(|error| match error {
        TokenError::InvalidGrant(message) => ApiError::InvalidGrant(message),
        TokenError::Upstream(message) => ApiError::Upstream(message),
    })?;

    Ok(Json(json!({
        "accessToken": tokens.access_token.expose_secret(),
        "expiresIn": tokens.expires_in,
        // Present only when the provider rotated it; the instance must keep the new one.
        "refreshToken": tokens.refresh_token.as_ref().map(ExposeSecret::expose_secret),
    })))
}
