// Copyright © 2026 Jalapeno Labs

//! Registering an MCP client through the relay (#195), against a fake of Hydra's public API.
//!
//! Hydra answers a registration with every optional field the client left out filled in as `""`
//! or `null`, and the MCP SDK inside Claude Code refuses that answer (`client_uri` must be a URL,
//! `contacts` an array), so Claude Code could never connect. The client must get Hydra's answer
//! without those fields and with everything it needs to sign in, and Hydra must get the request
//! as the client sent it, with nothing else.

use std::sync::{Arc, Mutex, MutexGuard};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, post};
use bytes::Bytes;
use serde_json::{Value, json};
use tower::ServiceExt as _;
use url::Url;

use super::router;
use crate::oauth::registration::Registration;

/// The registration access token the fake issues, which managing a registration needs.
const REGISTRATION_TOKEN: &str = "ory_at_registration-token";

/// What Claude Code sends to register: no website, logo, contacts, or policies.
fn claude_code_registration() -> Value {
    json!({
        "client_name": "Claude Code (elysium)",
        "redirect_uris": ["http://localhost:51234/callback"],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    })
}

/// What Hydra stored and answered for that request, as production answered it: every optional
/// field the client left out is present, empty.
fn hydra_answer(public: &Url) -> Value {
    json!({
        "client_id": "0d9a8c2e-5b8f-4a43-9b1f-7d3c2e8f1a65",
        "client_name": "Claude Code (elysium)",
        "redirect_uris": ["http://localhost:51234/callback"],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "scope": "offline_access workspace:read workspace:write",
        "token_endpoint_auth_method": "none",
        "subject_type": "public",
        "userinfo_signed_response_alg": "none",
        "client_secret_expires_at": 0,
        "created_at": "2026-11-09T12:00:00Z",
        "updated_at": "2026-11-09T12:00:00Z",
        "registration_access_token": REGISTRATION_TOKEN,
        "registration_client_uri":
            format!("{public}oauth2/register/0d9a8c2e-5b8f-4a43-9b1f-7d3c2e8f1a65"),
        "audience": [],
        "allowed_cors_origins": [],
        "post_logout_redirect_uris": [],
        "jwks": {},
        "metadata": {},
        "skip_consent": false,
        "owner": "",
        "policy_uri": "",
        "tos_uri": "",
        "client_uri": "",
        "logo_uri": "",
        "request_object_signing_alg": "",
        "contacts": null,
        "skip_logout_consent": null,
        "authorization_code_grant_access_token_lifespan": null,
        "authorization_code_grant_refresh_token_lifespan": null,
        "refresh_token_grant_access_token_lifespan": null,
    })
}

/// One request that reached the fake Hydra.
#[derive(Debug, Clone)]
struct Seen {
    method: Method,
    client_id: Option<String>,
    headers: HeaderMap,
    body: Bytes,
}

/// Hydra's public registration endpoint, recording every request that reaches it.
#[derive(Clone, Default)]
struct FakeHydra {
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl FakeHydra {
    fn seen(&self) -> MutexGuard<'_, Vec<Seen>> {
        self.seen.lock().expect("the fake's lock is never poisoned")
    }

    fn router(&self) -> Router {
        Router::new()
            .route("/oauth2/register", post(hydra_register))
            .route("/oauth2/register/{client_id}", delete(hydra_delete))
            .with_state(self.clone())
    }
}

/// Registers like Hydra: refuses a client without redirect URIs, and otherwise answers `201`
/// with the stored client, empty fields and all.
async fn hydra_register(
    State(hydra): State<FakeHydra>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    hydra.seen().push(Seen {
        method: Method::POST,
        client_id: None,
        headers,
        body: body.clone(),
    });

    let request: Value = serde_json::from_slice(&body).expect("the relay sent JSON");
    let has_redirect_uris = request["redirect_uris"]
        .as_array()
        .is_some_and(|uris| !uris.is_empty());
    if !has_redirect_uris {
        let refusal = json!({
            "error": "invalid_redirect_uri",
            "error_description": "The value of one or more redirect_uris is invalid.",
        });
        return (StatusCode::BAD_REQUEST, axum::Json(refusal)).into_response();
    }

    let public = Url::parse("https://work.example.com/").expect("a static URL parses");
    let no_store = [
        (header::CACHE_CONTROL, "no-store"),
        (header::PRAGMA, "no-cache"),
    ];
    (
        StatusCode::CREATED,
        no_store,
        axum::Json(hydra_answer(&public)),
    )
        .into_response()
}

/// Deletes a registration like Hydra: only with its registration access token.
async fn hydra_delete(
    State(hydra): State<FakeHydra>,
    Path(client_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let authorized = headers
        .get(header::AUTHORIZATION)
        .is_some_and(|value| value == format!("Bearer {REGISTRATION_TOKEN}").as_str());
    hydra.seen().push(Seen {
        method: Method::DELETE,
        client_id: Some(client_id),
        headers,
        body: Bytes::new(),
    });

    if !authorized {
        let refusal = json!({
            "error": "invalid_token",
            "error_description": "The registration access token is invalid.",
        });
        let challenge = [(header::WWW_AUTHENTICATE, "Bearer error=\"invalid_token\"")];
        return (StatusCode::UNAUTHORIZED, challenge, axum::Json(refusal)).into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

/// Serves `router` on a loopback port, and answers its address.
async fn serve(router: Router) -> Url {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port binds");
    let address = listener.local_addr().expect("the listener has an address");
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("the fake serves");
    });
    Url::parse(&format!("http://{address}/")).expect("a loopback URL parses")
}

/// The registration routes, relaying to a running fake Hydra.
async fn relay_to_fake() -> (Router, FakeHydra) {
    let hydra = FakeHydra::default();
    let public = serve(hydra.router()).await;
    let registration = Registration::new(reqwest::Client::new(), &public);
    (router(registration), hydra)
}

/// Sends one request through `routes`, and answers its status, headers, and body.
async fn send(routes: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, Bytes) {
    let response = routes
        .clone()
        .oneshot(request)
        .await
        .expect("the router answers");
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("the body reads");
    (status, headers, body)
}

fn register_request(body: &Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri("/register")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json")
        // Something a browser would send, which Hydra has no business reading.
        .header(header::COOKIE, "ory_kratos_session=a-session")
        .body(Body::from(
            serde_json::to_vec(body).expect("JSON serializes"),
        ))
        .expect("a request builds")
}

#[tokio::test]
async fn a_registration_answer_leaves_out_every_field_hydra_left_empty() {
    let (routes, hydra) = relay_to_fake().await;
    let registration = claude_code_registration();

    let (status, headers, body) = send(&routes, register_request(&registration)).await;

    assert_eq!(status, StatusCode::CREATED);
    let answer: Value = serde_json::from_slice(&body).expect("the answer is JSON");
    let fields = answer.as_object().expect("the answer is an object");
    let empty: Vec<&String> = fields
        .iter()
        .filter(|(_name, value)| value.is_null() || value.as_str() == Some(""))
        .map(|(name, _value)| name)
        .collect();
    assert!(
        empty.is_empty(),
        "empty fields reached the client: {empty:?}"
    );
    for absent in [
        "client_uri",
        "logo_uri",
        "tos_uri",
        "policy_uri",
        "owner",
        "contacts",
    ] {
        assert!(
            !fields.contains_key(absent),
            "{absent} is left out, not empty"
        );
    }

    // What the client needs to sign in and to manage its registration, as Hydra answered it.
    let hydra_said = hydra_answer(&Url::parse("https://work.example.com/").expect("a URL"));
    for kept in [
        "client_id",
        "redirect_uris",
        "registration_access_token",
        "registration_client_uri",
        "token_endpoint_auth_method",
        "scope",
        "client_secret_expires_at",
        "skip_consent",
        "audience",
        "jwks",
        "metadata",
    ] {
        assert_eq!(
            answer[kept], hydra_said[kept],
            "{kept} is relayed as Hydra sent it"
        );
    }
    assert_eq!(headers[header::CONTENT_TYPE], "application/json");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(headers[header::PRAGMA], "no-cache");

    let seen = hydra.seen();
    assert_eq!(seen.len(), 1, "one request reached Hydra");
    let request = &seen[0];
    assert_eq!(request.method, Method::POST);
    assert_eq!(
        serde_json::from_slice::<Value>(&request.body).expect("Hydra got JSON"),
        registration,
        "Hydra gets the registration as the client sent it"
    );
    assert_eq!(request.headers[header::CONTENT_TYPE], "application/json");
    assert_eq!(request.headers[header::ACCEPT], "application/json");
    assert!(
        !request.headers.contains_key(header::COOKIE),
        "only the headers Hydra reads are relayed"
    );
}

#[tokio::test]
async fn hydra_refusing_a_registration_is_relayed_with_its_reason() {
    let (routes, _hydra) = relay_to_fake().await;
    let mut registration = claude_code_registration();
    registration["redirect_uris"] = json!([]);

    let (status, _headers, body) = send(&routes, register_request(&registration)).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    let refusal: Value = serde_json::from_slice(&body).expect("the refusal is JSON");
    assert_eq!(refusal["error"], "invalid_redirect_uri");
    assert!(
        refusal["error_description"]
            .as_str()
            .is_some_and(|reason| !reason.is_empty())
    );
}

#[tokio::test]
async fn deleting_a_registration_carries_its_token_and_its_id_as_one_segment() {
    let (routes, hydra) = relay_to_fake().await;
    // An id that tries to climb out of the registration path must stay one segment of it.
    let delete_request = |authorization: Option<&str>| {
        let mut request = Request::builder()
            .method(Method::DELETE)
            .uri("/register/a-client%2F..%2F..%2Fadmin%2Fclients");
        if let Some(authorization) = authorization {
            request = request.header(header::AUTHORIZATION, authorization);
        }
        request.body(Body::empty()).expect("a request builds")
    };

    let (status, headers, body) = send(&routes, delete_request(None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "Hydra checks the token");
    assert_eq!(
        headers[header::WWW_AUTHENTICATE],
        "Bearer error=\"invalid_token\""
    );
    let refusal: Value = serde_json::from_slice(&body).expect("the refusal is JSON");
    assert_eq!(refusal["error"], "invalid_token");

    let bearer = format!("Bearer {REGISTRATION_TOKEN}");
    let (status, _headers, body) = send(&routes, delete_request(Some(&bearer))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty(), "a deletion answers nothing");

    let seen = hydra.seen();
    assert_eq!(seen.len(), 2, "both requests reached Hydra");
    let deleted = &seen[1];
    assert_eq!(deleted.method, Method::DELETE);
    assert_eq!(
        deleted.client_id.as_deref(),
        Some("a-client/../../admin/clients"),
        "the id reaches Hydra as one segment of its registration path"
    );
    assert_eq!(deleted.headers[header::AUTHORIZATION], bearer.as_str());
}

#[tokio::test]
async fn hydra_out_of_reach_answers_bad_gateway() {
    // Port 9 (discard) has nothing listening on loopback.
    let nowhere = Url::parse("http://127.0.0.1:9/").expect("a static URL parses");
    let routes = router(Registration::new(reqwest::Client::new(), &nowhere));

    let (status, _headers, _body) =
        send(&routes, register_request(&claude_code_registration())).await;

    assert_eq!(status, StatusCode::BAD_GATEWAY);
}
