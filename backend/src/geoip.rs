//! Best-effort IP geolocation for the session and sign-in views.
//!
//! Resolves a client IP to a coarse city / country so a user can answer "was this
//! me?" at a glance. Results are cached in Redis (the deployment's designated cache
//! layer) keyed by IP, so a repeated address costs no external call and the free
//! provider's rate limit is respected; Redis's own TTL expires stale entries with no
//! bookkeeping here.
//!
//! Lookups never block a request. A cache hit returns immediately; a miss returns
//! `None` (the caller shows the raw IP) and resolves in the background, so the next
//! read shows the location. The feature is entirely optional: with Redis unconfigured
//! or geolocation disabled, [`locate`] simply returns `None` and callers fall back to
//! the IP. Private, loopback, and link-local addresses never leave the deployment.
//!
//! Geolocation sends the client IP to a third-party service. That is a deliberate,
//! documented tradeoff (see `docs/geoip.md`); a self-hoster can disable it with
//! `ELYSIUM_GEOIP_ENABLED=false`, and a future local MaxMind database can replace the
//! remote call without changing this module's callers.

use std::net::IpAddr;
use std::time::Duration;

use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

use crate::state::AppState;

/// Free, keyless, HTTPS geolocation provider used unless overridden.
///
/// The trailing slash matters: the IP is appended directly (`https://ipwho.is/1.2.3.4`).
/// A different endpoint set via `ELYSIUM_GEOIP_ENDPOINT` must return the same JSON shape
/// (`success` / `city` / `country` / `country_code`); see [`ProviderResponse`].
pub const DEFAULT_ENDPOINT: &str = "https://ipwho.is/";

/// How long a resolved location stays cached.
///
/// Locations move rarely, so a long TTL keeps external lookups near zero. Redis expires
/// the key automatically, so a re-homed address refreshes within a month with no manual
/// invalidation.
const CACHE_TTL: Duration = Duration::from_secs(60 * 60 * 24 * 30);

/// How long a failed or empty lookup is remembered.
///
/// Short on purpose: a provider outage should not trigger a lookup on every page view,
/// but the address should get another chance within a day rather than never.
const NEGATIVE_TTL: Duration = Duration::from_secs(60 * 60 * 24);

/// How long to wait on the provider before giving up and showing the raw IP.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(3);

/// Cache value marking an address as known to have no location, distinct from a miss.
const NEGATIVE_MARKER: &str = "unknown";

/// A coarse, human-facing location for an IP: enough to recognize an unfamiliar sign-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub city: Option<String>,
    pub country: Option<String>,
    pub country_code: Option<String>,
}

/// The provider's response shape (ipwho.is and compatible services).
///
/// `success` is optional so a provider that omits it is treated as success; a `false`
/// value (ipwho.is returns it for reserved or invalid addresses) yields no location.
#[derive(Debug, Deserialize)]
struct ProviderResponse {
    success: Option<bool>,
    city: Option<String>,
    country: Option<String>,
    country_code: Option<String>,
}

/// Resolves the geolocation endpoint from the environment, or `None` when disabled.
///
/// Geolocation is on by default and disabled with `ELYSIUM_GEOIP_ENABLED=false`. The
/// endpoint defaults to [`DEFAULT_ENDPOINT`] and is overridable with
/// `ELYSIUM_GEOIP_ENDPOINT` for a compatible provider or a local proxy.
pub fn endpoint_from_env() -> Option<String> {
    let enabled = std::env::var("ELYSIUM_GEOIP_ENABLED")
        .map(|value| !value.eq_ignore_ascii_case("false"))
        .unwrap_or(true);
    if !enabled {
        return None;
    }
    let endpoint = std::env::var("ELYSIUM_GEOIP_ENDPOINT")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_owned());
    Some(endpoint)
}

/// Returns the cached location for `ip`, resolving a miss in the background.
///
/// Returns `None` when the address is absent, private, the feature is disabled, or the
/// location is not cached yet; the caller shows the raw IP in every such case. A miss on
/// a public address schedules a background lookup so the next read has the location.
pub async fn locate(state: &AppState, ip: Option<IpAddr>) -> Option<Location> {
    let ip = ip?;
    if state.geoip_endpoint.is_none() || !is_public(ip) {
        return None;
    }
    let mut redis = state.redis.clone()?;

    match redis.get::<_, Option<String>>(cache_key(ip)).await {
        Ok(Some(value)) if value == NEGATIVE_MARKER => None,
        Ok(Some(value)) => serde_json::from_str(&value).ok(),
        Ok(None) => {
            // Miss: resolve in the background so the list never waits on the network.
            spawn_resolve(state.clone(), ip);
            None
        }
        Err(error) => {
            tracing::warn!(error = %error, "geoip cache read failed");
            None
        }
    }
}

/// Schedules a one-off background resolve-and-cache for `ip`.
fn spawn_resolve(state: AppState, ip: IpAddr) {
    tokio::spawn(async move {
        let (value, ttl) = match fetch(&state, ip).await {
            Some(location) => (
                serde_json::to_string(&location).unwrap_or_else(|_| NEGATIVE_MARKER.to_owned()),
                CACHE_TTL,
            ),
            None => (NEGATIVE_MARKER.to_owned(), NEGATIVE_TTL),
        };
        let Some(mut redis) = state.redis.clone() else {
            return;
        };
        if let Err(error) = redis
            .set_ex::<_, _, ()>(cache_key(ip), value, ttl.as_secs())
            .await
        {
            tracing::warn!(error = %error, "geoip cache write failed");
        }
    });
}

/// Fetches a location for `ip` from the configured provider.
///
/// Returns `None` on a disabled feature, a network or timeout error, a non-success
/// response, or a response carrying neither a city nor a country.
async fn fetch(state: &AppState, ip: IpAddr) -> Option<Location> {
    let endpoint = state.geoip_endpoint.as_ref()?;
    let url = format!("{endpoint}{ip}");

    let response = state
        .http
        .get(&url)
        .timeout(LOOKUP_TIMEOUT)
        .send()
        .await
        .ok()?;
    let body = response.json::<ProviderResponse>().await.ok()?;
    parse(body)
}

/// Maps a provider response to a [`Location`], or `None` when it carries no place.
fn parse(body: ProviderResponse) -> Option<Location> {
    if !body.success.unwrap_or(true) {
        return None;
    }
    let location = Location {
        city: normalize(body.city),
        country: normalize(body.country),
        country_code: normalize(body.country_code),
    };
    if location.city.is_none() && location.country.is_none() {
        return None;
    }
    Some(location)
}

/// Trims a provider string and drops it when empty, so blanks never reach the client.
fn normalize(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// The Redis key for an address's cached location.
fn cache_key(ip: IpAddr) -> String {
    format!("geoip:{ip}")
}

/// Reports whether an address is routable on the public internet.
///
/// Private, loopback, link-local, and other non-global ranges resolve to no location
/// and, more importantly, are never sent to the external provider.
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified())
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() || v6.is_unspecified() {
                return false;
            }
            let first = v6.segments()[0];
            // Unique-local fc00::/7 and link-local fe80::/10 are not globally routable.
            let unique_local = (first & 0xfe00) == 0xfc00;
            let link_local = (first & 0xffc0) == 0xfe80;
            !(unique_local || link_local)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn private_and_loopback_addresses_are_not_public() {
        let cases = [
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)),
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)),
            IpAddr::V4(Ipv4Addr::new(172, 16, 3, 4)),
            IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1)),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1)),
            IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
        ];
        for ip in cases {
            assert!(!is_public(ip), "{ip} must not be treated as public");
        }
    }

    #[test]
    fn routable_addresses_are_public() {
        let cases = [
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
            IpAddr::V6(Ipv6Addr::new(0x2606, 0x4700, 0, 0, 0, 0, 0, 0x1111)),
        ];
        for ip in cases {
            assert!(is_public(ip), "{ip} must be treated as public");
        }
    }

    #[test]
    fn parses_a_successful_provider_response() {
        let body = ProviderResponse {
            success: Some(true),
            city: Some("Austin".to_owned()),
            country: Some("United States".to_owned()),
            country_code: Some("US".to_owned()),
        };
        let location = parse(body).expect("a place is present");
        assert_eq!(location.city.as_deref(), Some("Austin"));
        assert_eq!(location.country_code.as_deref(), Some("US"));
    }

    #[test]
    fn rejects_unsuccessful_or_empty_responses() {
        let failed = ProviderResponse {
            success: Some(false),
            city: Some("Nowhere".to_owned()),
            country: None,
            country_code: None,
        };
        assert!(parse(failed).is_none());

        let empty = ProviderResponse {
            success: Some(true),
            city: Some("   ".to_owned()),
            country: None,
            country_code: None,
        };
        assert!(parse(empty).is_none());
    }
}
