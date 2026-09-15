// Copyright © 2026 Jalapeno Labs

//! `POST /v1/redeem`: an instance's backend trades a handoff code for the account.
//!
//! The code alone is not enough. The caller must present the PKCE verifier whose
//! challenge it sent to `/v1/authorize`, which only the backend that started the flow
//! knows. A code read out of a browser history, a proxy log, or a shoulder is inert.

use std::time::SystemTime;

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, AppState, Handoff};
use crate::pkce;
use crate::sealing::Purpose;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RedeemRequest {
    handoff: String,
    code_verifier: String,
}

pub(super) async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RedeemRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(request) = body.map_err(|rejection| ApiError::BadRequest {
        code: "invalid_request",
        message: rejection.body_text(),
    })?;

    let handoff: Handoff = state
        .sealer
        .open(Purpose::Handoff, &request.handoff, SystemTime::now())
        .map_err(|error| ApiError::InvalidGrant(format!("handoff code {error}")))?;

    // Comparing challenges rather than secrets: learning how much of a challenge
    // matched does not help anyone find a verifier that hashes to it.
    if pkce::challenge_for(&request.code_verifier) != handoff.code_challenge {
        return Err(ApiError::InvalidGrant(
            "code verifier does not match".to_owned(),
        ));
    }

    Ok(Json(json!({
        "provider": handoff.provider,
        "address": handoff.address,
        "refreshToken": handoff.refresh_token,
    })))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use secrecy::SecretString;
    use tower::ServiceExt;

    use super::*;
    use crate::config::Config;
    use crate::providers::{AppCredentials, Provider};
    use crate::routes::router;
    use crate::sealing::{Sealer, generate_key};

    fn state() -> AppState {
        let config = Config {
            bind_address: "127.0.0.1:0".parse().expect("address"),
            public_url: "https://broker.example.com".parse().expect("url"),
            sealing_key: SecretString::from(generate_key()),
            google: Some(AppCredentials {
                client_id: "client".to_owned(),
                client_secret: SecretString::from("secret"),
            }),
            microsoft: None,
        };
        AppState {
            sealer: Arc::new(Sealer::from_base64_key(&config.sealing_key).expect("key")),
            config: Arc::new(config),
            http: reqwest::Client::new(),
        }
    }

    fn handoff_for(state: &AppState, verifier: &str) -> String {
        state.sealer.seal(
            Purpose::Handoff,
            &Handoff {
                provider: Provider::Google,
                address: "someone@gmail.com".to_owned(),
                refresh_token: "1//refresh".to_owned(),
                code_challenge: pkce::challenge_for(verifier),
            },
            Duration::from_secs(60),
            SystemTime::now(),
        )
    }

    async fn redeem(state: AppState, body: &Value) -> (StatusCode, Value) {
        let request = Request::post("/v1/redeem")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("request");
        let response = router(state).oneshot(request).await.expect("response");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        (status, serde_json::from_slice(&bytes).expect("json"))
    }

    #[tokio::test]
    async fn the_right_verifier_redeems_the_account() {
        let state = state();
        let verifier = pkce::new_verifier();
        let handoff = handoff_for(&state, &verifier);

        let (status, body) = redeem(
            state,
            &json!({ "handoff": handoff, "codeVerifier": verifier }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            json!({ "provider": "google", "address": "someone@gmail.com", "refreshToken": "1//refresh" })
        );
    }

    #[tokio::test]
    async fn a_handoff_code_without_its_verifier_is_inert() {
        let state = state();
        let handoff = handoff_for(&state, &pkce::new_verifier());

        let (status, body) = redeem(
            state,
            &json!({ "handoff": handoff, "codeVerifier": pkce::new_verifier() }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_grant");
        assert!(body.get("refreshToken").is_none());
    }

    #[tokio::test]
    async fn a_provider_state_token_is_not_a_handoff_code() {
        let state = state();
        let verifier = pkce::new_verifier();
        let wrong_purpose = state.sealer.seal(
            Purpose::ProviderState,
            &json!({ "anything": true }),
            Duration::from_secs(60),
            SystemTime::now(),
        );

        let (status, body) = redeem(
            state,
            &json!({ "handoff": wrong_purpose, "codeVerifier": verifier }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_grant");
    }
}
