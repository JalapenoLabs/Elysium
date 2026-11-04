// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/auth/sign-out`: end the session the request's cookie holds, and clear the cookie.
//!
//! The API signs out on the browser's behalf, rather than the browser asking Kratos, so the cached
//! session is forgotten in the same step (`crate::auth::sessions`). It needs no session of its
//! own: a session still waiting on its second factor signs out too, and so does one with no
//! account yet. A request with no session to end answers the same, since what it asked for is
//! already so.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::auth::kratos::KratosError;
use crate::auth::middleware::session_cookie;
use crate::auth::sessions;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let Some(cookie) = session_cookie(&headers) else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };

    let clearing = match sessions::sign_out(&state.redis, &state.auth.kratos, cookie).await {
        Ok(clearing) => clearing,
        Err(KratosError::NoSession) => Vec::new(),
        Err(error) => return Err(error.into()),
    };
    let mut response = StatusCode::NO_CONTENT.into_response();
    for value in clearing {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    Ok(response)
}
