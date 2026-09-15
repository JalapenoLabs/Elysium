// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/mail/oauth/{kind}/start`: send the browser to the broker to connect a
//! Gmail or Outlook account.
//!
//! The browser navigates here directly rather than calling it with `fetch`, because
//! the answer is a redirect it must follow. A fresh `state` and PKCE verifier are kept
//! in Redis until the callback; the verifier never reaches the browser. The `state`
//! is also set as an HTTP-only cookie, which the callback requires to match.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, header};
use axum::response::{AppendHeaders, IntoResponse, Redirect, Response};
use redis::AsyncCommands;
use secrecy::SecretString;
use url::Url;

use super::{
    OAUTH_CALLBACK_PATH, OAUTH_FLOW_COOKIE, OAUTH_FLOW_KEY_PREFIX, OAUTH_FLOW_TTL_SECONDS,
    PendingOAuthFlow,
};
use crate::errors::ApiError;
use crate::mail::broker::{pkce_challenge, random_token};
use crate::models::mail_account::MailAccountKind;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<MailAccountKind>, PathRejection>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let Path(kind) = path?;
    if kind == MailAccountKind::SelfHosted {
        return Err(ApiError::BadRequest(
            "self-hosted mailboxes are created, not connected".to_owned(),
        ));
    }
    let Some(broker) = state.mail.broker.as_ref() else {
        return Err(ApiError::Unavailable(
            "no OAuth broker is configured on this deployment",
        ));
    };

    let redirect_uri = callback_url(&headers)?;
    let flow_state = random_token();
    let code_verifier = random_token();
    let authorize_url = broker.authorize_url(
        kind,
        &redirect_uri,
        &flow_state,
        &pkce_challenge(&SecretString::from(code_verifier.clone())),
    )?;

    let pending = serde_json::to_string(&PendingOAuthFlow {
        kind,
        code_verifier,
    })
    .context("a pending OAuth flow serializes")?;
    let mut redis = state.redis.clone();
    let () = redis
        .set_ex(
            format!("{OAUTH_FLOW_KEY_PREFIX}{flow_state}"),
            pending,
            OAUTH_FLOW_TTL_SECONDS,
        )
        .await
        .context("cannot store the pending OAuth flow")?;

    // Lax, so the cookie still rides the top-level redirect back from the broker.
    let cookie = format!(
        "{OAUTH_FLOW_COOKIE}={flow_state}; Path={OAUTH_CALLBACK_PATH}; \
         Max-Age={OAUTH_FLOW_TTL_SECONDS}; HttpOnly; SameSite=Lax"
    );
    Ok((
        AppendHeaders([(header::SET_COOKIE, cookie)]),
        Redirect::to(authorize_url.as_str()),
    )
        .into_response())
}

/// The callback as the browser will reach it, rebuilt from the proxy's forwarding
/// headers. nginx is the only way to reach the API, and it always sets them.
fn callback_url(headers: &HeaderMap) -> Result<String, ApiError> {
    let header_text = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());

    let host = header_text("x-forwarded-host")
        .or_else(|| header_text(header::HOST.as_str()))
        .ok_or_else(|| ApiError::BadRequest("the request names no host".to_owned()))?;
    let scheme = match header_text("x-forwarded-proto") {
        Some("https") => "https",
        _ => "http",
    };

    // Parsing and comparing the path refuses a Host header that smuggles in a path or
    // credentials.
    let url = Url::parse(&format!("{scheme}://{host}{OAUTH_CALLBACK_PATH}"))
        .map_err(|_parse_error| ApiError::BadRequest("the request host is invalid".to_owned()))?;
    if url.path() != OAUTH_CALLBACK_PATH || !url.username().is_empty() || url.query().is_some() {
        return Err(ApiError::BadRequest(
            "the request host is invalid".to_owned(),
        ));
    }
    Ok(url.into())
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(*name, HeaderValue::from_static(value));
        }
        headers
    }

    #[test]
    fn the_callback_keeps_the_port_the_browser_used() {
        let forwarded = headers(&[
            ("host", "localhost"),
            ("x-forwarded-host", "localhost:4000"),
            ("x-forwarded-proto", "http"),
        ]);
        assert_eq!(
            callback_url(&forwarded).expect("valid"),
            "http://localhost:4000/api/v1/mail/oauth/callback"
        );

        let https = headers(&[
            ("host", "elysium.example.com"),
            ("x-forwarded-proto", "https"),
        ]);
        assert_eq!(
            callback_url(&https).expect("valid"),
            "https://elysium.example.com/api/v1/mail/oauth/callback"
        );
    }

    #[test]
    fn a_host_that_smuggles_a_path_or_credentials_is_refused() {
        for host in ["evil.example/steal?", "user@evil.example", "evil.example#"] {
            let smuggled = {
                let mut map = HeaderMap::new();
                map.insert("host", HeaderValue::from_str(host).expect("header"));
                map
            };
            assert!(callback_url(&smuggled).is_err(), "{host} must be refused");
        }
    }
}
