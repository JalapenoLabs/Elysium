// Copyright © 2026 Jalapeno Labs

//! `GET /v1/callback/{provider}`: where a provider returns the browser after consent.
//!
//! The broker opens its sealed state, exchanges the code with the provider using its
//! client secret, and sends the browser back to the instance with a handoff code. The
//! handoff holds the refresh token sealed, so it is opaque to the browser and to
//! anything that logs URLs along the way.
//!
//! Every outcome after the state opens is a redirect to the instance, carrying either
//! `handoff` or an OAuth-style `error`, so the instance always regains control of the
//! browser. Only a state that does not open ends here, on a problem page: without it
//! there is no validated address to return to.

use std::time::{Duration, SystemTime};

use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use secrecy::ExposeSecret;
use serde::Deserialize;
use tracing::{Level, event};
use url::Url;

use super::{AppState, Handoff, ProviderState};
use crate::pages;
use crate::providers::{self, Provider};
use crate::sealing::Purpose;

/// How long an instance has to redeem a handoff code. The redirect lands on the
/// instance's backend, which redeems immediately, so this only needs to cover latency.
const HANDOFF_LIFETIME: Duration = Duration::from_secs(120);

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

pub async fn handle(
    State(state): State<AppState>,
    Path(provider): Path<Provider>,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let opened = query.state.as_deref().map(|token| {
        state
            .sealer
            .open::<ProviderState>(Purpose::ProviderState, token, SystemTime::now())
    });
    let Some(Ok(provider_state)) = opened else {
        event!(
            name: "callback.state.rejected",
            Level::INFO,
            provider = ?provider,
            "callback without a valid state",
        );
        return pages::problem(
            "This sign-in link has expired",
            "It is older than fifteen minutes or was altered. Start connecting the account again from Elysium.",
        );
    };
    if provider_state.provider != provider {
        return pages::problem(
            "This sign-in link is invalid",
            "It was started for a different provider.",
        );
    }

    let return_to = |outcome: &[(&str, &str)]| -> Response {
        let mut url =
            Url::parse(&provider_state.redirect_uri).expect("sealed redirect URIs were validated");
        url.query_pairs_mut()
            .extend_pairs(outcome)
            .append_pair("state", &provider_state.instance_state);
        Redirect::to(url.as_str()).into_response()
    };

    if let Some(error) = query.error.as_deref() {
        return return_to(&[("error", error)]);
    }
    let Some(code) = query.code.as_deref() else {
        return return_to(&[("error", "invalid_request")]);
    };
    let Some(credentials) = state.config.credentials(provider) else {
        return return_to(&[("error", "provider_unavailable")]);
    };

    let tokens = match providers::exchange_code(
        &state.http,
        provider,
        credentials,
        code,
        &state.config.callback_url(provider),
        &provider_state.provider_verifier,
    )
    .await
    {
        Ok(tokens) => tokens,
        Err(error) => {
            event!(
                name: "callback.exchange.failure",
                Level::WARN,
                provider = ?provider,
                error.message = %error,
                "code exchange failed",
            );
            return return_to(&[("error", "token_exchange_failed")]);
        }
    };

    // Without a refresh token the instance could only use the mailbox for an hour.
    let Some(refresh_token) = tokens.refresh_token else {
        return return_to(&[("error", "no_refresh_token")]);
    };
    let Some(address) = tokens
        .id_token
        .as_deref()
        .and_then(providers::address_from_id_token)
    else {
        return return_to(&[("error", "no_address")]);
    };

    let handoff = state.sealer.seal(
        Purpose::Handoff,
        &Handoff {
            provider,
            address,
            refresh_token: refresh_token.expose_secret().to_owned(),
            code_challenge: provider_state.code_challenge.clone(),
        },
        HANDOFF_LIFETIME,
        SystemTime::now(),
    );

    event!(name: "callback.handoff.issued", Level::INFO, provider = ?provider, "handoff issued");
    return_to(&[("handoff", &handoff)])
}
