// Copyright © 2026 Jalapeno Labs

//! `GET /v1/authorize`: validate an instance's request and show the consent interstitial.
//!
//! Query parameters follow OAuth's own names: `provider`, `redirect_uri` (the
//! instance's callback), `state`, `code_challenge`, and `code_challenge_method`,
//! which must be `S256`.
//!
//! Nothing is redirected until the return address has been validated, so a malformed
//! request gets a problem page rather than a redirect somewhere unchecked.

use std::net::IpAddr;
use std::time::{Duration, SystemTime};

use axum::extract::{Query, State};
use axum::response::Response;
use serde::Deserialize;
use url::{Host, Url};

use super::{AppState, ProviderState};
use crate::pages;
use crate::pkce;
use crate::providers::Provider;
use crate::sealing::Purpose;

/// How long someone has to get through the interstitial and the provider's consent
/// screen. Long enough to sign in and approve with a second factor.
const CONSENT_WINDOW: Duration = Duration::from_mins(15);

/// Longest instance `state` the broker will carry. Elysium sends 43 characters.
const INSTANCE_STATE_MAX_LENGTH: usize = 512;

#[derive(Debug, Deserialize)]
pub struct AuthorizeQuery {
    provider: Provider,
    redirect_uri: String,
    state: String,
    code_challenge: String,
    code_challenge_method: String,
}

pub async fn handle(
    State(state): State<AppState>,
    Query(query): Query<AuthorizeQuery>,
) -> Response {
    let Some(credentials) = state.config.credentials(query.provider) else {
        return pages::problem(
            "Provider unavailable",
            "This broker does not offer that provider. Ask whoever runs it to configure one.",
        );
    };

    let redirect_uri = match validate_redirect_uri(&query.redirect_uri) {
        Ok(url) => url,
        Err(reason) => return pages::problem("This connection link is invalid", reason),
    };
    if query.code_challenge_method != "S256" || query.code_challenge.len() != pkce::CHALLENGE_LENGTH
    {
        return pages::problem(
            "This connection link is invalid",
            "It is missing a valid S256 PKCE challenge.",
        );
    }
    if query.state.is_empty() || query.state.len() > INSTANCE_STATE_MAX_LENGTH {
        return pages::problem(
            "This connection link is invalid",
            "Its state parameter is missing or too long.",
        );
    }

    let spec = query.provider.spec();
    let provider_verifier = pkce::new_verifier();
    let provider_challenge = pkce::challenge_for(&provider_verifier);
    let sealed_state = state.sealer.seal(
        Purpose::ProviderState,
        &ProviderState {
            provider: query.provider,
            redirect_uri: redirect_uri.to_string(),
            instance_state: query.state.clone(),
            code_challenge: query.code_challenge,
            provider_verifier,
        },
        CONSENT_WINDOW,
        SystemTime::now(),
    );

    let mut continue_url =
        Url::parse(spec.authorize_url).expect("provider authorize URLs are valid");
    continue_url
        .query_pairs_mut()
        .append_pair("client_id", &credentials.client_id)
        .append_pair("redirect_uri", &state.config.callback_url(query.provider))
        .append_pair("response_type", "code")
        .append_pair("scope", spec.scopes)
        .append_pair("state", &sealed_state)
        .append_pair("code_challenge", &provider_challenge)
        .append_pair("code_challenge_method", "S256")
        .extend_pairs(spec.extra_authorize_params);

    let mut cancel_url = redirect_uri.clone();
    cancel_url
        .query_pairs_mut()
        .append_pair("error", "access_denied")
        .append_pair("state", &query.state);

    pages::consent(
        &instance_label(&redirect_uri),
        spec.display_name,
        continue_url.as_str(),
        cancel_url.as_str(),
    )
}

/// Accepts a return address the broker is willing to send a handoff code to.
///
/// HTTPS anywhere. Plain HTTP only for loopback and private network addresses, which
/// is how most self-hosted instances are first reached (`http://localhost:4000`,
/// `http://192.168.1.20:4000`). A handoff code intercepted on such a network is still
/// useless without the instance's PKCE verifier, which never leaves its backend.
fn validate_redirect_uri(raw: &str) -> Result<Url, &'static str> {
    let url = Url::parse(raw).map_err(|_parse_error| "Its return address is not a URL.")?;

    if !url.username().is_empty() || url.password().is_some() {
        return Err("Its return address carries credentials.");
    }
    if url.fragment().is_some() {
        return Err("Its return address has a fragment.");
    }

    match url.scheme() {
        "https" => Ok(url),
        "http" if is_local_host(url.host().as_ref()) => Ok(url),
        "http" => {
            Err("Its return address uses plain HTTP on a public host. Serve Elysium over HTTPS.")
        }
        _ => Err("Its return address is not an HTTP URL."),
    }
}

fn is_local_host(host: Option<&Host<&str>>) -> bool {
    let address = match host {
        Some(Host::Domain(domain)) => return domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => IpAddr::V4(*address),
        Some(Host::Ipv6(address)) => IpAddr::V6(*address),
        None => return false,
    };

    match address {
        IpAddr::V4(address) => {
            address.is_loopback() || address.is_private() || address.is_link_local()
        }
        IpAddr::V6(address) => {
            address.is_loopback() || address.is_unique_local() || address.is_unicast_link_local()
        }
    }
}

/// The instance as the person signing in should recognize it: host, and port if it
/// is not the scheme's default.
fn instance_label(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_return_addresses_are_accepted_anywhere() {
        let url = validate_redirect_uri("https://elysium.example.com/api/v1/mail/oauth/callback")
            .expect("valid");
        assert_eq!(instance_label(&url), "elysium.example.com");
    }

    #[test]
    fn plain_http_is_accepted_only_on_local_networks() {
        for local in [
            "http://localhost:4000/cb",
            "http://127.0.0.1:4000/cb",
            "http://192.168.1.20:4000/cb",
            "http://10.0.0.5/cb",
            "http://[::1]:4000/cb",
            "http://[fd00::1]/cb",
        ] {
            validate_redirect_uri(local).unwrap_or_else(|reason| panic!("{local}: {reason}"));
        }

        for public in [
            "http://elysium.example.com/cb",
            "http://8.8.8.8/cb",
            "http://localhost.evil.com/cb",
        ] {
            assert!(
                validate_redirect_uri(public).is_err(),
                "{public} must be refused"
            );
        }
    }

    #[test]
    fn credentials_fragments_and_other_schemes_are_refused() {
        for invalid in [
            "https://user:pass@elysium.example.com/cb",
            "https://elysium.example.com/cb#fragment",
            "javascript:alert(1)",
            "ftp://elysium.example.com/cb",
            "not a url",
        ] {
            assert!(
                validate_redirect_uri(invalid).is_err(),
                "{invalid} must be refused"
            );
        }
    }

    #[test]
    fn the_label_keeps_a_non_default_port() {
        let url = validate_redirect_uri("http://192.168.1.20:4000/cb").expect("valid");
        assert_eq!(instance_label(&url), "192.168.1.20:4000");
    }
}
