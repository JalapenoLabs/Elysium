// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/me/sessions/{id}` and `DELETE /api/v1/me/sessions`: end one of the signed-in
//! person's other sessions, or all of them, from the Sessions tab of the Security page.
//!
//! The API ends them on the person's behalf, with the cookie of the session asking, rather than
//! the browser asking Kratos, so the cached sessions are forgotten in the same step and the ended
//! ones are refused on their next request (`crate::auth::sessions`). Kratos checks each session
//! is the person's. Like `/me`, these sit outside the workspace's access rule: ending one's own
//! sessions is never something to wait for an approval or an authenticator app to do.

use axum::Extension;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use uuid::Uuid;

use crate::auth::middleware::session_cookie;
use crate::auth::{AccessRefusal, Principal, sessions};
use crate::errors::ApiError;
use crate::state::AppState;

/// Ends one of the person's other sessions. The one asking signs out instead.
pub async fn end_one(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    headers: HeaderMap,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(session_id) = path?;
    if session_id == principal.session_id {
        return Err(ApiError::BadRequest(
            "this is the session you are using; sign out instead".to_owned(),
        ));
    }

    let cookie = cookie_of(&headers)?;
    sessions::end_session(
        &state.redis,
        &state.auth.kratos,
        cookie,
        principal.identity_id,
        session_id,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Ends every session of the person but the one asking.
pub async fn end_others(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let cookie = cookie_of(&headers)?;
    sessions::end_other_sessions(
        &state.redis,
        &state.auth.kratos,
        cookie,
        principal.identity_id,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The session cookie, which the auth layer found before these handlers ran.
fn cookie_of(headers: &HeaderMap) -> Result<&str, ApiError> {
    session_cookie(headers).ok_or(ApiError::AccessRefused(AccessRefusal::Unauthenticated))
}
