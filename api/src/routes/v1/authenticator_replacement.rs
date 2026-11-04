// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/me/authenticator-replacement` and `POST .../confirm`: moving the signed-in
//! person's two-factor authentication to a new authenticator app, keeping the current one until
//! a code from the new one is verified. See `crate::auth::authenticator_change`.
//!
//! These sit beside `/me` rather than behind the workspace's access rule, like the nickname: they
//! change how the person signs in, which nothing the workspace requires should stand in front of.
//! Both ask Kratos whose the session is directly and need a recent sign-in.

use anyhow::Context;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::{HeaderMap, StatusCode};
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};

use crate::auth::authenticator_change::{self, NewAuthenticator, REPLACEMENT_LIFETIME};
use crate::auth::kratos::Session;
use crate::auth::middleware::session_cookie;
use crate::auth::{AccessRefusal, Principal};
use crate::errors::ApiError;
use crate::models::authenticator_nickname::{self, NICKNAME_MAX_CHARACTERS};
use crate::models::authenticator_replacement;
use crate::state::AppState;

/// The new app to scan, or to type in by its key.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartResponse {
    /// A PNG `data:` URL.
    qr_code: String,
    secret_key: String,
    expires_at: DateTime<Utc>,
}

/// Starts moving to a new app: makes its secret and holds it, replacing a move started before.
/// The app in use keeps working.
pub async fn start(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    headers: HeaderMap,
) -> Result<Json<StartResponse>, ApiError> {
    let session = recent_session(&state, &headers).await?;
    require_an_app(&state, &principal).await?;

    let app = NewAuthenticator::generate(&session.identity.traits.email)
        .context("an email address holds no `:`")?;
    let expires_at = Utc::now() + REPLACEMENT_LIFETIME;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    authenticator_replacement::start(
        &mut connection,
        &state.cipher,
        principal.user.id,
        app.secret(),
        expires_at,
    )
    .await?;

    Ok(Json(StartResponse {
        qr_code: app.qr_code(),
        secret_key: app.secret_key(),
        expires_at,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfirmBody {
    /// A code the new app shows.
    code: String,
    /// What the person calls the new app.
    nickname: String,
}

/// Verifies a code from the new app, then makes it the person's app in place of the old one and
/// names it. Until this answers, the old app is the one in use.
pub async fn confirm(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    headers: HeaderMap,
    body: Result<Json<ConfirmBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(body) = body?;
    let nickname = body.nickname.trim();
    if nickname.is_empty() || nickname.chars().count() > NICKNAME_MAX_CHARACTERS {
        return Err(ApiError::BadRequest(format!(
            "a nickname is 1 to {NICKNAME_MAX_CHARACTERS} characters"
        )));
    }

    let session = recent_session(&state, &headers).await?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let secret = authenticator_replacement::find_secret(
        &mut connection,
        &state.cipher,
        principal.user.id,
        Utc::now(),
    )
    .await
    .context("could not read the new authenticator app")?
    .ok_or(ApiError::CodedConflict {
        code: "authenticator_replacement_expired",
        message: "the new app waited too long for its code; start again",
    })?;
    let app = NewAuthenticator::from_secret(secret.expose_secret(), &session.identity.traits.email)
        .context("a stored secret rebuilds its app")?;
    if !app.accepts(&body.code, Utc::now()) {
        return Err(ApiError::CodedBadRequest {
            code: "authenticator_code_invalid",
            message: "that code is not one the new app shows now".to_owned(),
        });
    }

    require_an_app(&state, &principal).await?;
    authenticator_change::replace(
        &state.redis,
        &state.auth.kratos,
        principal.identity_id,
        &app,
    )
    .await?;

    // The new app is in use from here, whatever follows: forgetting the held secret and naming
    // the app are Elysium's own records, and a failure in them is reported without undoing it.
    authenticator_replacement::clear(&mut connection, principal.user.id).await?;
    authenticator_nickname::set(&mut connection, principal.user.id, nickname).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The session the request came with, fresh from Kratos, if it signed in recently.
async fn recent_session(state: &AppState, headers: &HeaderMap) -> Result<Session, ApiError> {
    // The auth layer found this cookie before the handler ran.
    let cookie =
        session_cookie(headers).ok_or(ApiError::AccessRefused(AccessRefusal::Unauthenticated))?;
    authenticator_change::recent_session(&state.auth.kratos, cookie).await
}

/// Refuses unless Kratos holds an app for the person: there is nothing to move from otherwise,
/// and adding a first one goes through Kratos's settings flow.
async fn require_an_app(state: &AppState, principal: &Principal) -> Result<(), ApiError> {
    let methods = state
        .auth
        .kratos
        .identity(principal.identity_id)
        .await?
        .methods();
    if !methods.iter().any(|method| method == "totp") {
        return Err(ApiError::Conflict(
            "no authenticator app is set up to move from",
        ));
    }
    Ok(())
}
