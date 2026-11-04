// Copyright © 2026 Jalapeno Labs

//! `PUT` and `DELETE /api/v1/me/authenticator-nickname`: the name a person gives their own
//! authenticator app, such as "Bitwarden". See `crate::models::authenticator_nickname`.
//!
//! These sit beside `/me` rather than behind the workspace's access rule: someone the
//! workspace requires to set up an authenticator app names it while they set it up, before
//! their session counts as two-factor.

use anyhow::Context;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::Deserialize;

use crate::auth::Principal;
use crate::errors::ApiError;
use crate::models::authenticator_nickname::{self, NICKNAME_MAX_CHARACTERS};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    nickname: String,
}

/// Names the person's authenticator app. Kratos must hold one: a name for nothing would be
/// shown for the next app instead.
pub async fn set(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(body) = body?;
    let nickname = body.nickname.trim();
    if nickname.is_empty() || nickname.chars().count() > NICKNAME_MAX_CHARACTERS {
        return Err(ApiError::BadRequest(format!(
            "a nickname is 1 to {NICKNAME_MAX_CHARACTERS} characters"
        )));
    }

    let identity = principal
        .user
        .kratos_identity_id
        .context("a signed-in person always has a Kratos identity")?;
    let methods = state.auth.kratos.identity(identity).await?.methods();
    if !methods.iter().any(|method| method == "totp") {
        return Err(ApiError::Conflict("no authenticator app is set up to name"));
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    authenticator_nickname::set(&mut connection, principal.user.id, nickname).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Forgets the name, as when the person removes their authenticator app.
pub async fn clear(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Result<StatusCode, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    authenticator_nickname::clear(&mut connection, principal.user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}
