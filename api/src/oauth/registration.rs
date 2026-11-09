// Copyright © 2026 Jalapeno Labs

//! MCP clients' own registrations (RFC 7591 and RFC 7592), relayed to Hydra's public API.
//!
//! An MCP client such as Claude Code registers itself at `/oauth2/register`, the endpoint Hydra's
//! metadata advertises, and may read, update, or delete that registration at
//! `/oauth2/register/{client_id}` with the registration access token it was given. nginx sends
//! those paths to the API (`crate::routes::oauth2`), and [`Registration::relay`] forwards each
//! request to the same path on Hydra's public API as it came: its method, its body, and the
//! headers Hydra reads. Hydra's own rules for self-registered clients decide what is stored.
//!
//! The relay never reaches Hydra's admin API. That API accepts fields a client registering
//! itself must never set, such as `skip_consent`, which would let a client past the person's
//! consent.
//!
//! What the relay changes is Hydra's answer. Hydra fills every optional field the client did not
//! send with an empty value, `""` or `null`, and a client that validates the answer strictly
//! refuses it: the MCP SDK inside Claude Code requires `client_uri` to be a URL and `contacts` to
//! be an array whenever they are present. So the top-level fields of a JSON answer whose value is
//! `""` or `null` are left out, which RFC 7591 reads as not set. Everything else (the status, the
//! headers a client reads, empty arrays and objects) is relayed as Hydra sent it, and an answer
//! that names no cache policy is marked `no-store`, since it carries the registration access
//! token. Hydra tracks the empty fields as `ory/hydra#4044`; an answer with none passes through
//! unchanged.

use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, header};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use serde_json::Value;
use tracing::{Level, event};
use url::Url;

use super::hydra::{HydraError, REQUEST_TIMEOUT};

/// Where Hydra serves registration, relative to its public API.
const REGISTRATION_PATH: &str = "oauth2/register";

/// The request headers Hydra reads: what the body is, what answer is wanted, and, to manage a
/// registration, its registration access token. Nothing else the client sent reaches Hydra.
const RELAYED_REQUEST_HEADERS: [HeaderName; 3] =
    [header::CONTENT_TYPE, header::ACCEPT, header::AUTHORIZATION];

/// The answer headers a client reads: what the body is, how it may be cached, and why a management
/// call was refused. Headers about Hydra's connection or the body's length are never relayed,
/// since the body the client gets is not the one Hydra sent.
const RELAYED_RESPONSE_HEADERS: [HeaderName; 4] = [
    header::CONTENT_TYPE,
    header::CACHE_CONTROL,
    header::PRAGMA,
    header::WWW_AUTHENTICATE,
];

/// How an answer may be cached when Hydra does not say: never. A registration answer carries the
/// registration access token, and a secret for a confidential client, and Hydra's answers name no
/// cache policy of their own.
const NO_STORE: HeaderValue = HeaderValue::from_static("no-store");

/// Hydra's public registration endpoint, as the API reaches it on the compose network. Cloning
/// is cheap: it shares the HTTP client's pool.
#[derive(Debug, Clone)]
pub struct Registration {
    http: reqwest::Client,
    /// `/oauth2/register` on Hydra's public API.
    endpoint: Url,
}

impl Registration {
    /// Relays to the registration endpoint of the Hydra public API at `public`.
    pub fn new(http: reqwest::Client, public: &Url) -> Self {
        let endpoint = public
            .join(REGISTRATION_PATH)
            .expect("a relative path joins onto a base URL");
        Self { http, endpoint }
    }

    /// Forwards one request to Hydra, at the registration endpoint or at `client_id`'s
    /// registration under it, and answers what Hydra answered without its empty fields.
    ///
    /// Hydra's refusals, such as an invalid redirect URI or an unknown registration access token,
    /// are answers like any other: they come back with Hydra's status and reason.
    ///
    /// # Errors
    /// Returns [`HydraError::Refused`] when Hydra cannot be reached or its answer cannot be read.
    pub async fn relay(
        &self,
        method: Method,
        client_id: Option<&str>,
        headers: &HeaderMap,
        body: Bytes,
    ) -> Result<Response, HydraError> {
        let mut url = self.endpoint.clone();
        if let Some(client_id) = client_id {
            // One encoded segment, as Hydra's admin client builds its client URLs, so an id can
            // never name another path.
            url.path_segments_mut()
                .expect("an http URL has path segments")
                .push(client_id);
        }

        let mut request = self
            .http
            .request(method, url)
            .timeout(REQUEST_TIMEOUT)
            .body(body);
        for name in RELAYED_REQUEST_HEADERS {
            for value in headers.get_all(&name) {
                request = request.header(&name, value);
            }
        }
        let answer = request
            .send()
            .await
            .map_err(|error| HydraError::Refused(error.without_url().to_string()))?;

        let status = answer.status();
        let mut relayed_headers = HeaderMap::new();
        for name in RELAYED_RESPONSE_HEADERS {
            for value in answer.headers().get_all(&name) {
                relayed_headers.append(&name, value.clone());
            }
        }
        relayed_headers
            .entry(header::CACHE_CONTROL)
            .or_insert(NO_STORE);
        let body = answer
            .bytes()
            .await
            .map_err(|error| HydraError::Refused(format!("unreadable answer: {error}")))?;

        // The headers replace what the body would set on its own, so Hydra's `Content-Type` is the
        // one the client gets, never the `application/octet-stream` of a bare byte body.
        Ok((status, relayed_headers, without_empty_fields(body)).into_response())
    }
}

/// Leaves out the top-level fields of a JSON object whose value is `""` or `null`, and answers any
/// other body as it came: empty, such as a deleted registration's `204`, or not an object.
fn without_empty_fields(body: Bytes) -> Bytes {
    if body.is_empty() {
        return body;
    }

    let Ok(Value::Object(mut fields)) = serde_json::from_slice::<Value>(&body) else {
        event!(
            name: "oauth.registration.unfiltered",
            Level::DEBUG,
            http.response.body.size = body.len(),
            "Hydra's registration answer is not a JSON object, so it is relayed as it came",
        );
        return body;
    };

    fields.retain(|_name, value| !value.is_null() && value.as_str() != Some(""));
    let filtered = serde_json::to_vec(&fields).expect("a JSON object always serializes");
    Bytes::from(filtered)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn only_empty_strings_and_nulls_are_left_out() {
        let answer = json!({
            "client_id": "a-client",
            "client_uri": "",
            "contacts": null,
            "audience": [],
            "jwks": {},
            "metadata": { "nested": "" },
            "skip_consent": false,
            "client_secret_expires_at": 0,
        });
        let body = Bytes::from(serde_json::to_vec(&answer).expect("JSON serializes"));

        let filtered: Value =
            serde_json::from_slice(&without_empty_fields(body)).expect("the answer stays JSON");

        assert_eq!(
            filtered,
            json!({
                "client_id": "a-client",
                "audience": [],
                "jwks": {},
                "metadata": { "nested": "" },
                "skip_consent": false,
                "client_secret_expires_at": 0,
            }),
            "only top-level empty strings and nulls go; nested values, empty collections, false, \
             and zero stay"
        );
    }

    #[test]
    fn a_body_that_is_not_a_json_object_is_relayed_as_it_came() {
        for body in [
            "",
            "[\"\", null]",
            "\"\"",
            "null",
            "not json",
            "<html></html>",
        ] {
            let relayed = without_empty_fields(Bytes::from(body));
            assert_eq!(relayed, body.as_bytes(), "{body:?}");
        }
    }
}
