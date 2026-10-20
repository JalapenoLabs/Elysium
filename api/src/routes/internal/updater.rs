// Copyright © 2026 Jalapeno Labs

//! `/internal/updater`: what the deployment's updater calls. See `crate::updates`.
//!
//! The updater is a shell script with `wget` and no JSON parser, so both routes speak one
//! line of plain text. Each checks the updater's key, derived from the encryption key.
//!
//! - `GET /internal/updater/instruction` answers `wait` or `apply <target> from <running>`.
//! - `POST /internal/updater/outcome` takes `succeeded <version>` or
//!   `rolled-back <version> <reason>`, and answers `204`.

use axum::Router;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use secrecy::ExposeSecret;
use sha2::{Digest, Sha256};
use tracing::{Level, event};

use crate::auth::AccessRefusal;
use crate::errors::ApiError;
use crate::state::AppState;
use crate::updates::{Report, UPDATER_KEY_HEADER};

pub fn router(state: &AppState) -> Router<AppState> {
    Router::new()
        .route("/instruction", get(instruction))
        .route("/outcome", post(outcome))
        .route_layer(from_fn_with_state(state.clone(), require_updater_key))
}

async fn instruction(State(state): State<AppState>) -> Result<String, ApiError> {
    let instruction = state.updates.instruction().await?;
    Ok(instruction.to_string())
}

async fn outcome(State(state): State<AppState>, body: String) -> Result<StatusCode, ApiError> {
    let report = Report::parse(&body).map_err(ApiError::BadRequest)?;
    state.updates.report(report).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Refuses a call without the updater's key. Both sides are hashed before comparing, so the
/// comparison takes as long whatever the caller sent.
async fn require_updater_key(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let sent = request
        .headers()
        .get(UPDATER_KEY_HEADER)
        .map(|value| Sha256::digest(value.as_bytes()));
    let expected = Sha256::digest(state.updates.updater_key().expose_secret().as_bytes());
    if sent == Some(expected) {
        return next.run(request).await;
    }

    event!(
        name: "updates.updater.refused",
        Level::WARN,
        url.path = request.uri().path(),
        "refused an internal call without the updater's key",
    );
    ApiError::AccessRefused(AccessRefusal::Unauthenticated).into_response()
}
