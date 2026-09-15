// Copyright © 2026 Jalapeno Labs

//! The broker's HTTP surface.
//!
//! | Route                          | Caller              | Purpose                                  |
//! |--------------------------------|---------------------|------------------------------------------|
//! | `GET /healthz`                 | orchestrator        | liveness                                 |
//! | `GET /v1/providers`            | Elysium backend     | which providers this deployment offers   |
//! | `GET /v1/authorize`            | browser             | consent interstitial, then the provider  |
//! | `GET /v1/callback/{provider}`  | browser (provider)  | code exchange, handoff back to Elysium   |
//! | `POST /v1/redeem`              | Elysium backend     | handoff code + PKCE verifier for tokens  |
//! | `POST /v1/refresh`             | Elysium backend     | refresh token for an access token        |

mod authorize;
mod callback;
mod redeem;
mod refresh;

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{Level, event};

use crate::config::Config;
use crate::providers::{ALL_PROVIDERS, Provider};
use crate::sealing::Sealer;

/// Everything a handler can reach. Cloning is cheap: each field is a handle.
#[derive(Debug, Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub sealer: Arc<Sealer>,
    pub http: reqwest::Client,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/v1/providers", get(list_providers))
        .route("/v1/authorize", get(authorize::handle))
        .route("/v1/callback/{provider}", get(callback::handle))
        .route("/v1/redeem", post(redeem::handle))
        .route("/v1/refresh", post(refresh::handle))
        .with_state(state)
}

async fn list_providers(State(state): State<AppState>) -> Json<serde_json::Value> {
    let providers: Vec<Provider> = ALL_PROVIDERS
        .into_iter()
        .filter(|provider| state.config.credentials(*provider).is_some())
        .collect();
    Json(json!({ "providers": providers }))
}

/// What the provider-state token carries through the consent screen. Not `Debug`: it
/// holds a PKCE verifier.
#[derive(Serialize, Deserialize)]
struct ProviderState {
    provider: Provider,
    /// The Elysium callback, validated before it was sealed.
    redirect_uri: String,
    /// The instance's own `state`, returned untouched.
    instance_state: String,
    /// The instance's PKCE challenge, carried into the handoff.
    code_challenge: String,
    /// The broker's PKCE verifier towards the provider.
    provider_verifier: String,
}

/// What a handoff code carries to the instance. Not `Debug`: it holds a refresh token.
#[derive(Serialize, Deserialize)]
struct Handoff {
    provider: Provider,
    address: String,
    refresh_token: String,
    code_challenge: String,
}

/// A JSON API failure. Codes are stable; messages are for people.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{message}")]
    BadRequest { code: &'static str, message: String },
    /// The handoff code or refresh token is no good. Elysium must reconnect the account.
    #[error("{0}")]
    InvalidGrant(String),
    #[error("{0}")]
    Upstream(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::BadRequest { code, message } => (StatusCode::BAD_REQUEST, code, message),
            Self::InvalidGrant(message) => (StatusCode::BAD_REQUEST, "invalid_grant", message),
            Self::Upstream(message) => {
                event!(
                    name: "provider.request.failure",
                    Level::WARN,
                    error.message = %message,
                    "a provider call failed",
                );
                (StatusCode::BAD_GATEWAY, "upstream_failure", message)
            }
        };
        (status, Json(json!({ "error": code, "message": message }))).into_response()
    }
}
