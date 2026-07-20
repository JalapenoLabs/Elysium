//! The embedded HTTP reverse proxy that makes managed services reachable.
//!
//! A provisioned managed container (Forgejo, a registry, ...) runs on the host,
//! not inside Elysium. This module makes its web surface reachable and embeddable
//! *through* Elysium at a stable, single-origin URL: `GET /services/{id}/…` on the
//! backend forwards to the container that integration `id` owns. `docs/integrations.md`
//! (Routing) is the source of truth; the shape is summarized below.
//!
//! # Why an in-process proxy, not a managed Traefik/Caddy container
//!
//! The proxy lives in the same process as the Elysium session, which buys three
//! things a sidecar proxy container would not:
//!
//! - **Forward-auth needs no external round-trip.** [`authorize_proxy`] is the one
//!   choke point where #33 will require an Elysium session and assert identity to
//!   the upstream via trusted headers; it reads the session directly instead of
//!   bouncing every request out to an auth endpoint.
//! - **Single-origin embedding stays clean.** Everything is served from the
//!   Elysium origin under `/services/{id}`, so an embedded `<iframe>` faces no
//!   cross-origin cookie or CSP friction.
//! - **There is no extra container to provision, reconcile, or secure.**
//!
//! The tradeoff is that we handle streaming and connection upgrades ourselves. We
//! do so on `hyper` (a maintained HTTP library), never hand-rolled HTTP: the
//! upstream leg is a `hyper` client connection whose bodies stream through
//! untouched and whose `101 Switching Protocols` is tunnelled byte-for-byte.
//!
//! # Addressing and reachability
//!
//! A request addresses a service by integration id: `ANY /services/{id}/*path`.
//! The `/services/{id}` prefix is stripped and `/*path` (plus the query) is
//! forwarded upstream. Host-based routing (`git.example.com` → the git container)
//! is a later addition on top of the same [`RouteTable`]; this module is
//! path-based today.
//!
//! The backend reaches a managed container through the **host port that container
//! publishes**: the upstream is `http://host.docker.internal:{published_port}`.
//! `host.docker.internal` resolves to the host gateway (compose maps it on Linux;
//! Docker Desktop provides it already). The future alternative is to attach the
//! backend to the `elysium-managed` bridge network and route by container name.
//!
//! # Route lifecycle
//!
//! The [`RouteTable`] is desired state derived from integration state. The
//! reconciler ([`crate::orchestration::reconcile`]) registers a route when it
//! brings a managed integration with an HTTP surface up and unregisters it on
//! stop, removal, or a parked (pending/failed) reconcile, so a route appears and
//! disappears with its integration. Because the boot reconcile walks every managed
//! row, the table is rebuilt from integration state on startup.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, RwLock};

use axum::body::Body;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::header::HOST;
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use hyper::body::Incoming;
use hyper::client::conn::http1;
use hyper::upgrade::OnUpgrade;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use uuid::Uuid;

use crate::state::AppState;

/// The host the backend reaches a managed container's published port through.
///
/// A managed container publishes its web port on the deployment host; the backend
/// (itself a container) reaches that port at the host gateway. Compose maps this
/// name to the gateway on Linux, and Docker Desktop provides it already. Changing
/// this requires the matching `extra_hosts` entry on the `backend` service.
pub const MANAGED_UPSTREAM_HOST: &str = "host.docker.internal";

/// The proto/scheme Elysium presents to a managed upstream over the in-cluster hop.
///
/// The managed leg is plain HTTP; the operator terminates TLS in front of Elysium.
/// This is the value forwarded as `X-Forwarded-Proto` when the caller did not
/// already set one.
const FORWARDED_PROTO: &str = "http";

/// One HTTP route: an integration's id mapped to its upstream host and port.
///
/// Plain data, cheap to clone and compare. The upstream is always reached over
/// plain HTTP at `upstream_host:upstream_port` (see [`MANAGED_UPSTREAM_HOST`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// The managed integration this route forwards to.
    pub integration_id: Uuid,
    /// The host the upstream is reached at (the host gateway for a managed port).
    pub upstream_host: String,
    /// The published host port the upstream container listens on.
    pub upstream_port: u16,
}

impl Route {
    /// Creates a route from `integration_id` to `upstream_host:upstream_port`.
    pub fn new(integration_id: Uuid, upstream_host: impl Into<String>, upstream_port: u16) -> Self {
        Self {
            integration_id,
            upstream_host: upstream_host.into(),
            upstream_port,
        }
    }

    /// The `host:port` authority string used for the upstream `Host` header.
    pub fn authority(&self) -> String {
        format!("{}:{}", self.upstream_host, self.upstream_port)
    }
}

/// The live map of integration id to upstream, shared across the process.
///
/// Follows the `Arc<Inner>` service pattern (`docs/rust.md` M-SERVICES-CLONE), so
/// cloning it (into [`AppState`], into the reconciler) shares one table rather than
/// copying it. Reads (every proxied request) far outnumber writes (a reconcile), so
/// the interior is an `RwLock`: concurrent lookups never block one another.
#[derive(Clone, Default)]
pub struct RouteTable {
    inner: Arc<RwLock<HashMap<Uuid, Route>>>,
}

// Renders the route count only, never the map: a debug-formatted AppState must not
// dump the full routing table.
impl std::fmt::Debug for RouteTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RouteTable")
            .field("routes", &self.len())
            .finish()
    }
}

impl RouteTable {
    /// Creates an empty route table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `route`, replacing any existing route for the same integration.
    ///
    /// # Panics
    /// Panics only if the lock is poisoned, which signals a prior panic while a
    /// writer held it; that is an unrecoverable bug, not a runtime condition.
    pub fn register(&self, route: Route) {
        self.inner
            .write()
            .expect("route table lock poisoned")
            .insert(route.integration_id, route);
    }

    /// Removes the route for `integration_id`; a no-op if none is registered.
    ///
    /// # Panics
    /// Panics only if the lock is poisoned (see [`RouteTable::register`]).
    pub fn unregister(&self, integration_id: Uuid) {
        self.inner
            .write()
            .expect("route table lock poisoned")
            .remove(&integration_id);
    }

    /// Returns the route for `integration_id`, if one is registered.
    ///
    /// # Panics
    /// Panics only if the lock is poisoned (see [`RouteTable::register`]).
    pub fn lookup(&self, integration_id: Uuid) -> Option<Route> {
        self.inner
            .read()
            .expect("route table lock poisoned")
            .get(&integration_id)
            .cloned()
    }

    /// The number of registered routes.
    pub fn len(&self) -> usize {
        self.inner.read().expect("route table lock poisoned").len()
    }

    /// Whether no routes are registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The decision the forward-auth seam returns for a proxied request.
///
/// Today only [`ProxyAuth::allow`] is produced (open pass-through); the variants
/// are the shape #33 fills in. `Deny` short-circuits with a ready response (e.g.
/// a redirect to sign-in when no Elysium session is present); `Allow` carries the
/// trusted identity headers to inject upstream (empty until #33).
pub enum ProxyAuth {
    /// Proceed, injecting these trusted identity headers into the upstream request.
    Allow {
        /// Headers asserted to the upstream (e.g. `X-Elysium-User`), empty today.
        identity: HeaderMap,
    },
    /// Reject the request before proxying, returning this response to the caller.
    Deny(Response),
}

impl ProxyAuth {
    /// Allows the request through with no injected identity headers (today's default).
    pub fn allow() -> Self {
        Self::Allow {
            identity: HeaderMap::new(),
        }
    }

    /// Whether this decision permits the request to be proxied.
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }
}

/// The forward-auth choke point: authorizes a request before it is proxied.
///
/// Every proxied request passes through here exactly once, before the upstream
/// connection is opened. Today it allows everything, so a managed service is
/// reachable to any caller who can reach the backend. This is the single seam
/// #33 fills in: it will (a) require a valid Elysium session, denying with a
/// redirect otherwise, and (b) return the trusted identity headers
/// (`X-Elysium-User`, `X-Elysium-Email`) to assert the caller's identity to the
/// upstream, which forge/registry images consume as forward-auth. Keeping this a
/// function of `(&AppState, &Parts, integration_id)` means #33 has the session
/// cookie, the database handle, and the target integration in hand without
/// reshaping the proxy.
pub async fn authorize_proxy(
    _state: &AppState,
    _parts: &Parts,
    _integration_id: Uuid,
) -> ProxyAuth {
    // TODO(#33): resolve the Elysium session from `_parts` and `_state`, deny with
    // a sign-in redirect when absent, and populate the trusted identity headers.
    ProxyAuth::allow()
}

/// Proxy handler for `ANY /services/{integration_id}` (the service root).
pub async fn proxy_root(
    State(state): State<AppState>,
    Path(integration_id): Path<Uuid>,
    request: Request,
) -> Response {
    let peer = peer_ip(&request);
    forward(&state, peer, integration_id, String::new(), request).await
}

/// Proxy handler for `ANY /services/{integration_id}/*path` (everything under it).
pub async fn proxy(
    State(state): State<AppState>,
    Path((integration_id, tail)): Path<(Uuid, String)>,
    request: Request,
) -> Response {
    let peer = peer_ip(&request);
    forward(&state, peer, integration_id, tail, request).await
}

/// The remote peer's IP, read from the connection info the server attaches.
///
/// `None` when no `ConnectInfo` is present (e.g. an in-process test harness); the
/// forwarded-for header is simply omitted in that case.
fn peer_ip(request: &Request) -> Option<IpAddr> {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip())
}

/// Forwards one request to the integration's upstream, streaming the response.
///
/// Resolves the route (404 if the integration has none), runs the forward-auth
/// seam, rewrites headers, opens a `hyper` connection to the upstream, and either
/// streams the response body back or, on a `101`, tunnels the upgraded connection
/// both ways. Never buffers a whole body.
async fn forward(
    state: &AppState,
    peer: Option<IpAddr>,
    integration_id: Uuid,
    tail: String,
    request: Request,
) -> Response {
    let Some(route) = state.routes.lookup(integration_id) else {
        return (StatusCode::NOT_FOUND, "no route for this service").into_response();
    };

    let (mut parts, body) = request.into_parts();

    // The forward-auth choke point (#33 fills this in). A denial short-circuits.
    let identity = match authorize_proxy(state, &parts, integration_id).await {
        ProxyAuth::Deny(response) => return response,
        ProxyAuth::Allow { identity } => identity,
    };

    let upgrading = is_upgrade_request(&parts.headers);
    // Take the client-side upgrade future now, before the request is consumed. It
    // resolves once we return `101` to the caller, handing us the upgraded socket.
    let client_on_upgrade = if upgrading {
        parts.extensions.remove::<OnUpgrade>()
    } else {
        None
    };

    let path_and_query = upstream_path_and_query(&tail, parts.uri.query());
    let uri = match build_upstream_uri(&route, &path_and_query) {
        Ok(uri) => uri,
        Err(_) => return (StatusCode::BAD_GATEWAY, "invalid upstream target").into_response(),
    };

    // Rebuild the forwardable header set: strip hop-by-hop, add X-Forwarded-*, set
    // the upstream Host, then layer the trusted identity headers over the top.
    let mut headers = forwardable_headers(&parts.headers, upgrading);
    apply_forwarded_headers(
        &mut headers,
        peer,
        parts.headers.get(HOST),
        &route.authority(),
    );
    for (name, value) in &identity {
        headers.insert(name.clone(), value.clone());
    }

    // Stream the request body straight through; nothing is collected.
    let mut upstream_request = Request::new(body);
    *upstream_request.method_mut() = parts.method.clone();
    *upstream_request.uri_mut() = uri;
    *upstream_request.headers_mut() = headers;

    // Open a fresh HTTP/1 connection to the upstream. A per-request connection is
    // simple and correct for embedded UIs; pooling is a later optimization.
    let stream = match TcpStream::connect((route.upstream_host.as_str(), route.upstream_port)).await
    {
        Ok(stream) => stream,
        Err(error) => {
            tracing::warn!(
                integration.id = %integration_id,
                upstream = %route.authority(),
                error = %error,
                "proxy upstream connect failed",
            );
            return (StatusCode::BAD_GATEWAY, "upstream unreachable").into_response();
        }
    };
    let (mut sender, connection) = match http1::handshake(TokioIo::new(stream)).await {
        Ok(pair) => pair,
        Err(error) => {
            tracing::warn!(error = %error, "proxy upstream handshake failed");
            return (StatusCode::BAD_GATEWAY, "upstream handshake failed").into_response();
        }
    };
    // Drive the connection in the background, with upgrades enabled so a tunnelled
    // websocket's `hyper::upgrade::on` resolves.
    tokio::spawn(async move {
        if let Err(error) = connection.with_upgrades().await {
            tracing::debug!(error = %error, "proxy upstream connection closed");
        }
    });

    let response = match sender.send_request(upstream_request).await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(error = %error, "proxy upstream request failed");
            return (StatusCode::BAD_GATEWAY, "upstream request failed").into_response();
        }
    };

    // A `101` means the upstream accepted the upgrade: tunnel both halves.
    if upgrading && response.status() == StatusCode::SWITCHING_PROTOCOLS {
        return spawn_upgrade_tunnel(response, client_on_upgrade);
    }

    // Ordinary response: hand the streaming upstream body back, stripping the
    // upstream's hop-by-hop headers so our own framing is authoritative.
    let filtered = forwardable_headers(response.headers(), false);
    let (mut response_parts, incoming) = response.into_parts();
    response_parts.headers = filtered;
    Response::from_parts(response_parts, Body::new(incoming))
}

/// Tunnels an upgraded (`101`) upstream connection to the client, both directions.
///
/// Spawns a task that awaits both upgraded sockets and copies bytes between them
/// until either side closes, then returns the `101` response to the caller so the
/// server upgrades the client leg. When the client upgrade future is missing (no
/// serving upgrade support), the `101` is still returned but no tunnel runs.
fn spawn_upgrade_tunnel(
    mut response: Response<Incoming>,
    client_on_upgrade: Option<OnUpgrade>,
) -> Response {
    // Take the upstream-side upgrade future before the response is consumed; it
    // resolves once the background connection completes the upgrade handshake.
    let upstream_on_upgrade = hyper::upgrade::on(&mut response);

    if let Some(client_on_upgrade) = client_on_upgrade {
        tokio::spawn(async move {
            match tokio::try_join!(client_on_upgrade, upstream_on_upgrade) {
                Ok((client_upgraded, upstream_upgraded)) => {
                    let mut client_io = TokioIo::new(client_upgraded);
                    let mut upstream_io = TokioIo::new(upstream_upgraded);
                    if let Err(error) =
                        tokio::io::copy_bidirectional(&mut client_io, &mut upstream_io).await
                    {
                        tracing::debug!(error = %error, "proxy websocket tunnel closed");
                    }
                }
                Err(error) => {
                    tracing::debug!(error = %error, "proxy websocket upgrade failed");
                }
            }
        });
    }

    // Relay the switch to the caller: same 101 status and (filtered) handshake
    // headers, empty body. The serving connection performs the client-side upgrade.
    let filtered = forwardable_headers(response.headers(), true);
    let mut switching = Response::new(Body::empty());
    *switching.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
    *switching.headers_mut() = filtered;
    switching
}

/// Builds the origin-form path and query to forward, from the stripped `*path`.
///
/// The router already removed the `/services/{id}` prefix, leaving `tail` as the
/// remaining path without a leading slash (empty at the service root). This
/// re-adds the leading slash and re-attaches the original query, so
/// `/services/{id}/a/b?x=1` forwards as `/a/b?x=1` and `/services/{id}` as `/`.
fn upstream_path_and_query(tail: &str, query: Option<&str>) -> String {
    let mut out = String::with_capacity(tail.len() + 2);
    out.push('/');
    out.push_str(tail);
    if let Some(query) = query {
        out.push('?');
        out.push_str(query);
    }
    out
}

/// Builds the absolute upstream URI from a route and an origin-form path+query.
///
/// # Errors
/// Returns an error if the assembled URI is malformed (e.g. an unparseable path).
fn build_upstream_uri(route: &Route, path_and_query: &str) -> Result<Uri, axum::http::Error> {
    Uri::builder()
        .scheme(FORWARDED_PROTO)
        .authority(route.authority())
        .path_and_query(path_and_query)
        .build()
}

/// Whether a request is a protocol-upgrade (e.g. websocket) handshake.
///
/// True when a `Connection` header lists the `upgrade` token and an `Upgrade`
/// header is present, matching how a server decides to upgrade.
fn is_upgrade_request(headers: &HeaderMap) -> bool {
    let connection_upgrades = headers
        .get(axum::http::header::CONNECTION)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
        })
        .unwrap_or(false);
    connection_upgrades && headers.contains_key(axum::http::header::UPGRADE)
}

/// Whether a header is hop-by-hop and must not cross a proxy boundary.
///
/// The RFC 7230 hop-by-hop set plus every `Proxy-*` header. `Connection` and
/// `Upgrade` are in the set but are re-added by the caller when an upgrade is in
/// flight (see [`forwardable_headers`]). Header names arrive lowercased, so the
/// comparisons are exact.
fn is_hop_by_hop(name: &HeaderName) -> bool {
    let name = name.as_str();
    matches!(
        name,
        "connection" | "keep-alive" | "te" | "trailer" | "transfer-encoding" | "upgrade"
    ) || name.starts_with("proxy-")
}

/// Copies `headers` into the set that may cross the proxy, dropping hop-by-hop ones.
///
/// Drops the fixed hop-by-hop set, every `Proxy-*` header, and any header a peer
/// explicitly marked hop-by-hop by listing it in its `Connection` header. When
/// `upgrading` is true, `Connection` and `Upgrade` survive so the handshake
/// reaches the other side. `X-Forwarded-*` and `Host` are set by
/// [`apply_forwarded_headers`], not here.
fn forwardable_headers(headers: &HeaderMap, upgrading: bool) -> HeaderMap {
    // Tokens the peer named in `Connection` are hop-by-hop for this hop only.
    let connection_tokens: Vec<String> = headers
        .get(axum::http::header::CONNECTION)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(',')
                .map(|token| token.trim().to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default();

    let mut out = HeaderMap::new();
    for (name, value) in headers {
        if is_hop_by_hop(name) {
            // Preserve the upgrade handshake headers when upgrading.
            if upgrading && matches!(name.as_str(), "connection" | "upgrade") {
                out.append(name.clone(), value.clone());
            }
            continue;
        }
        // A header the peer named in `Connection` is hop-by-hop, unless it is the
        // upgrade token during an upgrade (already handled above for `Upgrade`).
        if connection_tokens
            .iter()
            .any(|token| token.as_str() == name.as_str())
        {
            continue;
        }
        out.append(name.clone(), value.clone());
    }
    out
}

/// Adds the `X-Forwarded-*` and upstream `Host` headers to a forwardable set.
///
/// `X-Forwarded-For` appends the peer IP to any existing chain; `X-Forwarded-Host`
/// records the caller's original `Host`; `X-Forwarded-Proto` is set only when the
/// caller did not already provide one (the operator's TLS front proxy may have).
/// `Host` is set to the upstream authority so the HTTP/1.1 request line is
/// well-formed for the upstream container.
fn apply_forwarded_headers(
    headers: &mut HeaderMap,
    peer: Option<IpAddr>,
    original_host: Option<&HeaderValue>,
    upstream_authority: &str,
) {
    let x_forwarded_for = HeaderName::from_static("x-forwarded-for");
    let x_forwarded_host = HeaderName::from_static("x-forwarded-host");
    let x_forwarded_proto = HeaderName::from_static("x-forwarded-proto");

    if let Some(peer) = peer {
        let chained = match headers.get(&x_forwarded_for).and_then(|v| v.to_str().ok()) {
            Some(existing) => format!("{existing}, {peer}"),
            None => peer.to_string(),
        };
        if let Ok(value) = HeaderValue::from_str(&chained) {
            headers.insert(x_forwarded_for, value);
        }
    }

    if let Some(host) = original_host {
        headers.insert(x_forwarded_host, host.clone());
    }

    if !headers.contains_key(&x_forwarded_proto) {
        headers.insert(x_forwarded_proto, HeaderValue::from_static(FORWARDED_PROTO));
    }

    if let Ok(host) = HeaderValue::from_str(upstream_authority) {
        headers.insert(HOST, host);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route() -> Route {
        Route::new(Uuid::new_v4(), MANAGED_UPSTREAM_HOST, 38080)
    }

    // --- RouteTable ----------------------------------------------------------

    #[test]
    fn route_table_registers_looks_up_and_unregisters() {
        let table = RouteTable::new();
        assert!(table.is_empty());

        let route = route();
        let id = route.integration_id;
        table.register(route.clone());

        assert_eq!(table.len(), 1);
        assert_eq!(table.lookup(id), Some(route));

        table.unregister(id);
        assert_eq!(table.lookup(id), None);
        assert!(table.is_empty());
    }

    #[test]
    fn register_replaces_the_route_for_the_same_integration() {
        let table = RouteTable::new();
        let id = Uuid::new_v4();
        table.register(Route::new(id, MANAGED_UPSTREAM_HOST, 1000));
        table.register(Route::new(id, MANAGED_UPSTREAM_HOST, 2000));

        // One integration keeps exactly one route; the newer upstream wins.
        assert_eq!(table.len(), 1);
        assert_eq!(table.lookup(id).map(|r| r.upstream_port), Some(2000));
    }

    #[test]
    fn unregister_of_an_absent_integration_is_a_noop() {
        let table = RouteTable::new();
        table.unregister(Uuid::new_v4());
        assert!(table.is_empty());
    }

    #[test]
    fn route_authority_is_host_colon_port() {
        assert_eq!(route().authority(), "host.docker.internal:38080");
    }

    #[test]
    fn route_table_debug_hides_the_map() {
        let table = RouteTable::new();
        table.register(route());
        let rendered = format!("{table:?}");
        // The count is shown; no upstream host or port leaks into debug output.
        assert!(rendered.contains("routes: 1"));
        assert!(!rendered.contains("host.docker.internal"));
    }

    // --- Path / prefix strip -------------------------------------------------

    #[test]
    fn upstream_path_and_query_re_adds_slash_and_query() {
        assert_eq!(upstream_path_and_query("a/b", Some("x=1")), "/a/b?x=1");
    }

    #[test]
    fn upstream_path_and_query_maps_the_root_to_slash() {
        assert_eq!(upstream_path_and_query("", None), "/");
    }

    #[test]
    fn upstream_path_and_query_without_a_query_omits_the_question_mark() {
        assert_eq!(
            upstream_path_and_query("assets/app.js", None),
            "/assets/app.js"
        );
    }

    #[test]
    fn build_upstream_uri_assembles_scheme_authority_and_path() {
        let route = Route::new(Uuid::new_v4(), MANAGED_UPSTREAM_HOST, 38080);
        let uri = build_upstream_uri(&route, "/a/b?x=1").expect("valid uri");
        assert_eq!(uri.to_string(), "http://host.docker.internal:38080/a/b?x=1");
    }

    // --- Upgrade detection ---------------------------------------------------

    #[test]
    fn is_upgrade_request_detects_a_websocket_handshake() {
        let mut headers = HeaderMap::new();
        headers.insert("connection", HeaderValue::from_static("Upgrade"));
        headers.insert("upgrade", HeaderValue::from_static("websocket"));
        assert!(is_upgrade_request(&headers));
    }

    #[test]
    fn is_upgrade_request_handles_a_multi_token_connection_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "connection",
            HeaderValue::from_static("keep-alive, Upgrade"),
        );
        headers.insert("upgrade", HeaderValue::from_static("websocket"));
        assert!(is_upgrade_request(&headers));
    }

    #[test]
    fn is_upgrade_request_is_false_without_both_headers() {
        let mut only_connection = HeaderMap::new();
        only_connection.insert("connection", HeaderValue::from_static("upgrade"));
        assert!(!is_upgrade_request(&only_connection));

        let mut plain = HeaderMap::new();
        plain.insert("connection", HeaderValue::from_static("keep-alive"));
        assert!(!is_upgrade_request(&plain));
    }

    // --- Hop-by-hop stripping ------------------------------------------------

    #[test]
    fn forwardable_headers_strips_hop_by_hop_and_keeps_end_to_end() {
        let mut headers = HeaderMap::new();
        headers.insert("connection", HeaderValue::from_static("keep-alive"));
        headers.insert("keep-alive", HeaderValue::from_static("timeout=5"));
        headers.insert("transfer-encoding", HeaderValue::from_static("chunked"));
        headers.insert("te", HeaderValue::from_static("trailers"));
        headers.insert("proxy-authorization", HeaderValue::from_static("secret"));
        headers.insert("content-type", HeaderValue::from_static("text/html"));
        headers.insert("cookie", HeaderValue::from_static("session=abc"));

        let out = forwardable_headers(&headers, false);

        // End-to-end headers survive; every hop-by-hop / proxy header is gone.
        assert_eq!(out.get("content-type").unwrap(), "text/html");
        assert_eq!(out.get("cookie").unwrap(), "session=abc");
        assert!(out.get("connection").is_none());
        assert!(out.get("keep-alive").is_none());
        assert!(out.get("transfer-encoding").is_none());
        assert!(out.get("te").is_none());
        assert!(out.get("proxy-authorization").is_none());
    }

    #[test]
    fn forwardable_headers_drops_connection_named_headers() {
        // A peer may name extra hop-by-hop headers in Connection; those must drop too.
        let mut headers = HeaderMap::new();
        headers.insert("connection", HeaderValue::from_static("x-custom-hop"));
        headers.insert("x-custom-hop", HeaderValue::from_static("nope"));
        headers.insert("x-keep", HeaderValue::from_static("yes"));

        let out = forwardable_headers(&headers, false);
        assert!(out.get("x-custom-hop").is_none());
        assert_eq!(out.get("x-keep").unwrap(), "yes");
    }

    #[test]
    fn forwardable_headers_preserves_the_upgrade_handshake_when_upgrading() {
        let mut headers = HeaderMap::new();
        headers.insert("connection", HeaderValue::from_static("Upgrade"));
        headers.insert("upgrade", HeaderValue::from_static("websocket"));
        headers.insert(
            "sec-websocket-key",
            HeaderValue::from_static("dGhlIHNhbXBsZQ=="),
        );

        // Upgrading keeps Connection + Upgrade so the handshake reaches the upstream.
        let upgrading = forwardable_headers(&headers, true);
        assert_eq!(upgrading.get("connection").unwrap(), "Upgrade");
        assert_eq!(upgrading.get("upgrade").unwrap(), "websocket");
        assert_eq!(
            upgrading.get("sec-websocket-key").unwrap(),
            "dGhlIHNhbXBsZQ=="
        );

        // Not upgrading, the same headers are hop-by-hop and stripped.
        let plain = forwardable_headers(&headers, false);
        assert!(plain.get("connection").is_none());
        assert!(plain.get("upgrade").is_none());
    }

    // --- X-Forwarded-* injection --------------------------------------------

    #[test]
    fn apply_forwarded_headers_sets_the_full_forwarded_set() {
        let mut headers = HeaderMap::new();
        let peer: IpAddr = "203.0.113.7".parse().unwrap();
        let host = HeaderValue::from_static("elysium.example.com");

        apply_forwarded_headers(
            &mut headers,
            Some(peer),
            Some(&host),
            "host.docker.internal:38080",
        );

        assert_eq!(headers.get("x-forwarded-for").unwrap(), "203.0.113.7");
        assert_eq!(
            headers.get("x-forwarded-host").unwrap(),
            "elysium.example.com"
        );
        assert_eq!(headers.get("x-forwarded-proto").unwrap(), "http");
        // Host is rewritten to the upstream so its request line is well-formed.
        assert_eq!(headers.get("host").unwrap(), "host.docker.internal:38080");
    }

    #[test]
    fn apply_forwarded_headers_appends_to_an_existing_forwarded_for_chain() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("198.51.100.1"));
        let peer: IpAddr = "203.0.113.7".parse().unwrap();

        apply_forwarded_headers(&mut headers, Some(peer), None, "up:80");
        assert_eq!(
            headers.get("x-forwarded-for").unwrap(),
            "198.51.100.1, 203.0.113.7"
        );
    }

    #[test]
    fn apply_forwarded_headers_keeps_an_existing_proto() {
        // The operator's TLS front proxy already set https; we must not clobber it.
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        apply_forwarded_headers(&mut headers, None, None, "up:80");
        assert_eq!(headers.get("x-forwarded-proto").unwrap(), "https");
    }

    // --- Forward-auth seam ---------------------------------------------------

    #[test]
    fn proxy_auth_allow_is_permissive_and_carries_no_identity_today() {
        match ProxyAuth::allow() {
            ProxyAuth::Allow { identity } => assert!(identity.is_empty()),
            ProxyAuth::Deny(_) => panic!("allow() must permit the request"),
        }
        assert!(ProxyAuth::allow().is_allowed());
    }

    // --- Websocket tunnel byte copy ------------------------------------------
    //
    // A full websocket round-trip needs a live ws upstream; no such image is
    // pinned into the suite, so the tunnel's byte-copy core is exercised directly
    // over in-memory duplex streams. The proxy wires the two real upgraded sockets
    // into this same `copy_bidirectional`, so this covers the moving part.

    #[tokio::test]
    async fn upgrade_tunnel_copies_bytes_in_both_directions() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // Two ends of the "client" pipe and two of the "upstream" pipe. The tunnel
        // copies between the proxy-facing ends; the test drives the far ends.
        let (mut client_far, client_near) = tokio::io::duplex(64);
        let (mut upstream_far, upstream_near) = tokio::io::duplex(64);

        let tunnel = tokio::spawn(async move {
            let mut client_near = client_near;
            let mut upstream_near = upstream_near;
            tokio::io::copy_bidirectional(&mut client_near, &mut upstream_near).await
        });

        // Client -> upstream.
        client_far.write_all(b"ping").await.expect("write client");
        let mut buf = [0u8; 4];
        upstream_far
            .read_exact(&mut buf)
            .await
            .expect("read upstream");
        assert_eq!(&buf, b"ping");

        // Upstream -> client.
        upstream_far
            .write_all(b"pong")
            .await
            .expect("write upstream");
        let mut buf = [0u8; 4];
        client_far.read_exact(&mut buf).await.expect("read client");
        assert_eq!(&buf, b"pong");

        // Closing both far ends lets the bidirectional copy finish cleanly.
        drop(client_far);
        drop(upstream_far);
        tunnel.await.expect("join tunnel").expect("tunnel copy");
    }

    // --- Live end-to-end proxy (ignored, needs Docker) ----------------------
    //
    // Mirrors the docker.rs live test: runs only when ELYSIUM_DOCKER_PROXY_URL
    // points at a real (proxied) engine, exercised in CI via
    // `cargo test -- --ignored` inside the backend container (where
    // host.docker.internal resolves to the host gateway). It provisions a pinned
    // echo upstream, proxies a real HTTP GET through the router, and asserts the
    // route add/remove behavior. A full websocket round-trip needs a live ws
    // upstream that we do not pin into the suite; the upgrade tunnel's byte-copy
    // core is covered by `upgrade_tunnel_copies_bytes_in_both_directions` above.

    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use http_body_util::BodyExt;
    use opaque_ke::ServerSetup;
    use rand::rngs::OsRng;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use crate::crypto::Crypto;
    use crate::opaque::Suite;
    use crate::orchestration::provisioner::{
        PortBind, PortProtocol, Provisioner, ProvisionerHandle, ServiceSpec,
    };
    use crate::sse::SseRegistry;

    /// The pinned echo upstream: `traefik/whoami` reflects the request it received.
    ///
    /// Pinned by tag (never `latest`), matching the docker.rs live test's style.
    /// Overridable via `ELYSIUM_PROXY_TEST_IMAGE` for a digest pin in CI.
    const WHOAMI_IMAGE: &str = "traefik/whoami:v1.10.2";

    /// The host port the echo upstream publishes; the proxy dials it via the gateway.
    const WHOAMI_HOST_PORT: u16 = 39080;

    /// Builds an [`AppState`] whose only exercised field is the route table.
    ///
    /// The proxy path touches no database, so the pool is lazy and never connects;
    /// the auth/crypto handles exist only to satisfy the struct.
    fn proxy_test_state(provisioner: ProvisionerHandle) -> AppState {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://tester:tester@localhost:1/elysia_test")
            .expect("a lazy pool never connects at build time");
        AppState {
            pool,
            sse: SseRegistry::default(),
            webauthn: crate::webauthn::build("localhost", "http://localhost:5173")
                .expect("the test WebAuthn RP builds"),
            opaque_setup: Arc::new(ServerSetup::<Suite>::new(&mut OsRng)),
            crypto: Crypto::new(&[0u8; 32]).expect("a 32-byte test key is valid"),
            redis_url: None,
            redis: None,
            http: reqwest::Client::new(),
            geoip_endpoint: None,
            rabbitmq_url: None,
            app_origin: "http://localhost:5173".to_string(),
            data_dir: "/srv/elysium".to_string(),
            provisioner,
            routes: RouteTable::new(),
        }
    }

    #[tokio::test]
    #[ignore = "requires a Docker engine via ELYSIUM_DOCKER_PROXY_URL"]
    async fn proxies_http_to_a_live_upstream_and_tracks_the_route() {
        let Some(proxy_url) = std::env::var("ELYSIUM_DOCKER_PROXY_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
        else {
            return;
        };
        let image = std::env::var("ELYSIUM_PROXY_TEST_IMAGE")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| WHOAMI_IMAGE.to_string());

        // Provision the echo upstream, publishing container port 80 on the host.
        let provisioner =
            ProvisionerHandle::docker(&proxy_url, Vec::new()).expect("connect docker backend");
        let name = format!("elysium-proxytest-{}", Uuid::new_v4());
        let mut spec = ServiceSpec::new(&name, &image);
        spec.ports.push(PortBind {
            host_port: WHOAMI_HOST_PORT,
            container_port: 80,
            protocol: PortProtocol::Tcp,
        });

        // Run the assertions with guaranteed cleanup of the container afterward.
        let outcome = run_live_proxy(&provisioner, &spec).await;
        provisioner.remove(&name).await.expect("cleanup remove");
        outcome.expect("live proxy assertions");
    }

    /// Drives the register -> proxy -> unregister flow against a live upstream.
    async fn run_live_proxy(
        provisioner: &ProvisionerHandle,
        spec: &ServiceSpec,
    ) -> Result<(), String> {
        provisioner
            .ensure(spec)
            .await
            .map_err(|error| format!("ensure upstream: {error}"))?;

        let state = proxy_test_state(provisioner.clone());
        let integration_id = Uuid::new_v4();
        state.routes.register(Route::new(
            integration_id,
            MANAGED_UPSTREAM_HOST,
            WHOAMI_HOST_PORT,
        ));

        let app = crate::api::router(state.clone());
        let path = format!("/services/{integration_id}");

        // whoami needs a moment to start listening; retry the first proxied GET.
        let (status, body) = get_with_retry(&app, &path).await?;
        if status != StatusCode::OK {
            return Err(format!("expected 200 through the proxy, got {status}"));
        }
        // whoami echoes the request it saw; the forwarded header proves our rewrite
        // reached the upstream, and the echo proves the body streamed back.
        if !body.contains("Hostname:") {
            return Err(format!("upstream echo not found in body: {body}"));
        }
        if !body.contains("X-Forwarded-Host") {
            return Err(format!("forwarded headers not seen upstream: {body}"));
        }

        // Unregistering the route makes the service unreachable through Elysium.
        state.routes.unregister(integration_id);
        let app = crate::api::router(state.clone());
        let response = app
            .oneshot(
                HttpRequest::get(&path)
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router responds");
        if response.status() != StatusCode::NOT_FOUND {
            return Err(format!(
                "expected 404 after unregister, got {}",
                response.status()
            ));
        }
        Ok(())
    }

    /// Sends `GET path` through the router, retrying while the upstream boots.
    async fn get_with_retry(
        app: &axum::Router,
        path: &str,
    ) -> Result<(StatusCode, String), String> {
        for attempt in 0..20 {
            let response = app
                .clone()
                .oneshot(
                    HttpRequest::get(path)
                        .body(Body::empty())
                        .expect("valid request"),
                )
                .await
                .expect("router responds");
            let status = response.status();
            let bytes = response
                .into_body()
                .collect()
                .await
                .expect("read body")
                .to_bytes();
            let body = String::from_utf8_lossy(&bytes).into_owned();
            if status == StatusCode::OK {
                return Ok((status, body));
            }
            // 502 while the container is still starting: back off and retry.
            if attempt == 19 {
                return Ok((status, body));
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        Err("upstream never became reachable".to_string())
    }
}
