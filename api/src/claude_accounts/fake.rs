// Copyright © 2026 Jalapeno Labs

//! A local fake of Anthropic's OAuth endpoints, with state, for tests of the client and keeper.
//!
//! It answers the way the real endpoints do for the calls Elysium makes. The token endpoint
//! rotates: each refresh with the current refresh token answers a new pair and retires the old
//! token, and any other token is refused with `400 invalid_grant`, exactly as a copy of an
//! already rotated `.credentials.json` is. Profile and usage need a bearer the fake issued.
//!
//! No live Anthropic account exists here, so these prove Elysium's half of the contract against
//! the shapes Claude Code relies on, not against Anthropic.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use uuid::Uuid;

use super::oauth::Anthropic;

#[derive(Debug)]
struct FakeState {
    /// The one refresh token the fake accepts now.
    refresh_token: String,
    /// Every access token the fake issued, and the one it starts with.
    access_tokens: Vec<String>,
    refreshes: usize,
    /// When set, the token endpoint answers `503`, as during an outage.
    is_down: bool,
    usage: Value,
    token_requests: Vec<Value>,
}

/// A running fake and a client pointed at it.
#[derive(Clone)]
pub(crate) struct FakeAnthropic {
    state: Arc<Mutex<FakeState>>,
    origin: String,
}

impl FakeAnthropic {
    /// The refresh token the fake accepts before its first rotation.
    pub(crate) const REFRESH_TOKEN: &'static str = "refresh-initial";
    /// An access token the fake accepts from the start.
    pub(crate) const ACCESS_TOKEN: &'static str = "access-initial";
    /// The account every token belongs to.
    pub(crate) const ACCOUNT_UUID: Uuid =
        Uuid::from_u128(0x0193_4f2a_7c1e_7d00_8000_0000_0000_0001);

    /// Starts the fake on a free local port.
    pub(crate) async fn start() -> Self {
        let state = Arc::new(Mutex::new(FakeState {
            refresh_token: Self::REFRESH_TOKEN.to_owned(),
            access_tokens: vec![Self::ACCESS_TOKEN.to_owned()],
            refreshes: 0,
            is_down: false,
            usage: json!({
                "five_hour": { "utilization": 12.0, "resets_at": "2099-01-01T05:00:00Z" },
                "seven_day": { "utilization": 3.0, "resets_at": "2099-01-07T00:00:00Z" },
            }),
            token_requests: Vec::new(),
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let origin = format!("http://{}", listener.local_addr().expect("address"));
        let router = Router::new()
            .fallback(answer)
            .with_state(Arc::clone(&state));
        tokio::spawn(async move { axum::serve(listener, router).await });
        Self { state, origin }
    }

    /// A client pointed at the fake.
    pub(crate) fn client(&self) -> Anthropic {
        Anthropic::with_test_origin(reqwest::Client::new(), &self.origin)
    }

    fn with_state<Output>(&self, act: impl FnOnce(&mut FakeState) -> Output) -> Output {
        act(&mut self.state.lock().expect("lock"))
    }

    /// How many refreshes succeeded.
    pub(crate) fn refreshes(&self) -> usize {
        self.with_state(|state| state.refreshes)
    }

    /// The refresh token the fake accepts now.
    pub(crate) fn current_refresh_token(&self) -> String {
        self.with_state(|state| state.refresh_token.clone())
    }

    /// How many token requests arrived, refused or not.
    pub(crate) fn token_requests(&self) -> usize {
        self.with_state(|state| state.token_requests.len())
    }

    /// The body of the last token request.
    pub(crate) fn last_token_request(&self) -> Option<Value> {
        self.with_state(|state| state.token_requests.last().cloned())
    }

    /// Makes the token endpoint answer `503` until called again with `false`.
    pub(crate) fn set_down(&self, is_down: bool) {
        self.with_state(|state| state.is_down = is_down);
    }

    /// Changes what the usage endpoint answers.
    pub(crate) fn set_usage(&self, usage: Value) {
        self.with_state(|state| state.usage = usage);
    }
}

async fn answer(
    State(state): State<Arc<Mutex<FakeState>>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let mut state = state.lock().expect("lock");
    match (method, uri.path()) {
        (Method::POST, "/v1/oauth/token") => {
            let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            state.token_requests.push(request.clone());
            if state.is_down {
                return (StatusCode::SERVICE_UNAVAILABLE, "upstream down").into_response();
            }
            if request["refresh_token"] != state.refresh_token.as_str() {
                return (
                    StatusCode::BAD_REQUEST,
                    axum::Json(json!({
                        "error": "invalid_grant",
                        "error_description": "Refresh token not found or invalid",
                    })),
                )
                    .into_response();
            }
            state.refreshes += 1;
            let access_token = format!("access-rotated-{}", state.refreshes);
            state.refresh_token = format!("refresh-rotated-{}", state.refreshes);
            state.access_tokens.push(access_token.clone());
            axum::Json(json!({
                "access_token": access_token,
                "refresh_token": state.refresh_token,
                "expires_in": 8 * 3600,
                "token_type": "Bearer",
            }))
            .into_response()
        }
        (Method::GET, path @ ("/api/oauth/profile" | "/api/oauth/usage")) => {
            let bearer = headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
                .unwrap_or_default();
            if !state.access_tokens.iter().any(|token| token == bearer)
                || headers.get("anthropic-beta").is_none()
            {
                return (
                    StatusCode::UNAUTHORIZED,
                    axum::Json(json!({ "error": "invalid_token" })),
                )
                    .into_response();
            }
            if path == "/api/oauth/profile" {
                return axum::Json(json!({
                    "account": {
                        "uuid": FakeAnthropic::ACCOUNT_UUID,
                        "email": "alex@example.com",
                        "display_name": "Alex",
                    },
                    "organization": { "uuid": Uuid::nil(), "name": "Jalapeno Labs" },
                }))
                .into_response();
            }
            axum::Json(state.usage.clone()).into_response()
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}
