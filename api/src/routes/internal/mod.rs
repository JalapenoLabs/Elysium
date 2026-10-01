// Copyright © 2026 Jalapeno Labs

//! `/internal`: what Kratos calls. nginx proxies nothing here (it forwards `/api/` alone), so
//! only services on the compose network reach these routes, and each checks the hook key
//! Kratos carries as well. See `docs/auth.md`.

mod courier;
mod registered;
mod registration;

use axum::Router;
use axum::extract::{Request, State};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use secrecy::ExposeSecret;
use sha2::{Digest, Sha256};
use tracing::{Level, event};

use crate::auth::AccessRefusal;
use crate::auth::hook_key::HOOK_KEY_HEADER;
use crate::errors::ApiError;
use crate::state::AppState;

pub fn router(state: &AppState) -> Router<AppState> {
    Router::new()
        .route("/kratos/registration", post(registration::handle))
        .route("/kratos/registered", post(registered::handle))
        .route("/kratos/courier", post(courier::handle))
        .route_layer(from_fn_with_state(state.clone(), require_hook_key))
}

/// Refuses a call without Kratos's hook key. Both sides are hashed before comparing, so the
/// comparison takes as long whatever the caller sent.
async fn require_hook_key(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let sent = request
        .headers()
        .get(HOOK_KEY_HEADER)
        .map(|value| Sha256::digest(value.as_bytes()));
    let expected = Sha256::digest(state.auth.hook_key.expose_secret().as_bytes());
    if sent == Some(expected) {
        return next.run(request).await;
    }

    event!(
        name: "auth.hook.refused",
        Level::WARN,
        url.path = request.uri().path(),
        "refused an internal call without Kratos's hook key",
    );
    ApiError::AccessRefused(AccessRefusal::Unauthenticated).into_response()
}
