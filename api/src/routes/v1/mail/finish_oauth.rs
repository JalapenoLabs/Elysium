// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/mail/oauth/callback`: where the broker returns the browser.
//!
//! The flow must match both the `state` stored when it started and the cookie set on
//! the browser that started it, and each flow is taken out of Redis exactly once. The
//! handoff code is then redeemed server to server with the PKCE verifier, and the
//! account is created, or reconnected when its address is already here.
//!
//! Every outcome is a redirect back to the mail settings page, carrying either
//! `mailConnected=<id>` or `mailError=<code>`, because a person's browser is waiting.

use anyhow::Context;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, header};
use axum::response::{AppendHeaders, IntoResponse, Redirect, Response};
use redis::AsyncCommands;
use secrecy::SecretString;
use serde::Deserialize;
use tracing::{Level, event};
use url::form_urlencoded;

use super::{
    MAIL_SETTINGS_PATH, OAUTH_CALLBACK_PATH, OAUTH_FLOW_COOKIE, OAUTH_FLOW_KEY_PREFIX,
    PendingOAuthFlow,
};
use crate::errors::ApiError;
use crate::models::mail_account::{self, MailAccount, NewMailAccount};
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    state: Option<String>,
    handoff: Option<String>,
    error: Option<String>,
}

pub async fn handle(
    State(state): State<AppState>,
    Query(query): Query<CallbackQuery>,
    headers: HeaderMap,
) -> Response {
    let outcome = match complete(&state, &query, &headers).await {
        Ok(account) => ("mailConnected", account.id.to_string()),
        Err(FlowError::Refused(code)) => ("mailError", code.to_owned()),
        Err(FlowError::Api(error)) => {
            event!(
                name: "mail.oauth.failure",
                Level::WARN,
                error.message = %error,
                "an OAuth connection could not be completed",
            );
            ("mailError", error_code(&error).to_owned())
        }
    };

    let destination = format!(
        "{MAIL_SETTINGS_PATH}?{}",
        form_urlencoded::Serializer::new(String::new())
            .append_pair(outcome.0, &outcome.1)
            .finish()
    );
    // The flow is over whichever way it went; the cookie has nothing left to match.
    let expired_cookie = format!(
        "{OAUTH_FLOW_COOKIE}=; Path={OAUTH_CALLBACK_PATH}; Max-Age=0; HttpOnly; SameSite=Lax"
    );
    (
        AppendHeaders([(header::SET_COOKIE, expired_cookie)]),
        Redirect::to(&destination),
    )
        .into_response()
}

enum FlowError {
    /// A stable code for the settings page to explain.
    Refused(&'static str),
    Api(ApiError),
}

impl<Source: Into<ApiError>> From<Source> for FlowError {
    fn from(error: Source) -> Self {
        Self::Api(error.into())
    }
}

async fn complete(
    state: &AppState,
    query: &CallbackQuery,
    headers: &HeaderMap,
) -> Result<MailAccount, FlowError> {
    let Some(flow_state) = query.state.as_deref() else {
        return Err(FlowError::Refused("invalid_request"));
    };
    if cookie_value(headers, OAUTH_FLOW_COOKIE) != Some(flow_state) {
        return Err(FlowError::Refused("state_mismatch"));
    }

    let mut redis = state.redis.clone();
    let pending: Option<String> = redis
        .get_del(format!("{OAUTH_FLOW_KEY_PREFIX}{flow_state}"))
        .await
        .context("cannot read the pending OAuth flow")?;
    let Some(pending) = pending else {
        return Err(FlowError::Refused("flow_expired"));
    };
    let pending: PendingOAuthFlow =
        serde_json::from_str(&pending).context("a stored OAuth flow does not parse")?;

    if let Some(error) = query.error.as_deref() {
        return Err(FlowError::Refused(provider_error_code(error)));
    }
    let Some(handoff) = query.handoff.as_deref() else {
        return Err(FlowError::Refused("invalid_request"));
    };
    let Some(broker) = state.mail.broker.as_ref() else {
        return Err(FlowError::Refused("broker_unavailable"));
    };

    let redeemed = broker
        .redeem(
            pending.kind,
            handoff,
            &SecretString::from(pending.code_verifier),
        )
        .await?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let account = match mail_account::find_by_address(&mut connection, &redeemed.address).await? {
        Some(existing) if existing.kind != pending.kind => {
            return Err(FlowError::Refused("address_in_use"));
        }
        Some(existing) => {
            mail_account::replace_credential(
                &mut connection,
                &state.cipher,
                existing.id,
                &redeemed.refresh_token,
            )
            .await?
        }
        None => {
            let new_account = NewMailAccount {
                kind: pending.kind,
                address: redeemed.address,
                display_name: String::new(),
                credential: redeemed.refresh_token,
                external_id: None,
                mail_domain_id: None,
            };
            mail_account::create(&mut connection, &state.cipher, &new_account).await?
        }
    };

    event!(
        name: "mail.oauth.connected",
        Level::INFO,
        mail.account.id = %account.id,
        mail.account.kind = ?account.kind,
        "a mailbox was connected",
    );
    state
        .events
        .publish(&ServerEvent::MailAccountUpserted(account.clone().into()));
    Ok(account)
}

/// One cookie's value from the request's `Cookie` headers.
fn cookie_value<'headers>(headers: &'headers HeaderMap, name: &str) -> Option<&'headers str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| {
            let (key, value) = pair.trim().split_once('=')?;
            (key == name).then_some(value)
        })
}

/// Errors the broker relays from a provider or raises itself, passed through when the
/// settings page knows them and generalized otherwise.
fn provider_error_code(error: &str) -> &'static str {
    const KNOWN: [&str; 5] = [
        "access_denied",
        "token_exchange_failed",
        "no_refresh_token",
        "no_address",
        "provider_unavailable",
    ];
    KNOWN
        .into_iter()
        .find(|known| *known == error)
        .unwrap_or("provider_error")
}

fn error_code(error: &ApiError) -> &'static str {
    match error {
        ApiError::BadGateway(_) => "broker_refused",
        ApiError::Conflict(_) => "address_in_use",
        _ => "internal_error",
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    #[test]
    fn the_flow_cookie_is_found_among_others() {
        let mut headers = HeaderMap::new();
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("theme=dark; elysium_mail_oauth=abc123"),
        );
        headers.append(header::COOKIE, HeaderValue::from_static("other=1"));

        assert_eq!(cookie_value(&headers, OAUTH_FLOW_COOKIE), Some("abc123"));
        assert_eq!(cookie_value(&headers, "missing"), None);
    }

    #[test]
    fn unknown_provider_errors_are_generalized() {
        assert_eq!(provider_error_code("access_denied"), "access_denied");
        assert_eq!(provider_error_code("<script>"), "provider_error");
    }
}
