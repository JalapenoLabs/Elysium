// Copyright © 2026 Jalapeno Labs

//! A revoked session or MCP token is refused on its very next request (#188), though the API
//! caches what Kratos and Hydra said about it: signing out, ending one's other sessions, an admin
//! signing someone out, Kratos's own revocation on a settings change, disconnecting an MCP
//! client, and deleting one. Moving to a new authenticator app writes to Kratos's admin API,
//! which fires no webhook, so it forgets the person's sessions itself (#165). With Redis down, every request asks Kratos or Hydra instead and
//! nothing is refused for it. Since answers are cached for minutes (#190), a session in use still
//! rolls: it is extended as soon as it is due, whatever the cache holds.
//!
//! The router runs against Postgres and Redis, with Kratos and Hydra faked on loopback: each
//! fake counts how often it was asked, which proves an answer really was cached before the
//! revocation dropped it.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::{Form, Path, Query, State};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use chrono::Utc;
use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{Value, json};
use tower::ServiceExt as _;
use url::Url;
use uuid::Uuid;

use crate::auth::hook_key::HOOK_KEY_HEADER;
use crate::auth::kratos::{
    AssuranceLevel, Identity, Kratos, PersonName, SESSION_COOKIE, Session, Traits,
};
use crate::auth::middleware::session_cookie;
use crate::auth::sessions::SESSION_LIFESPAN;
use crate::oauth::hydra::{Hydra, Introspection};
use crate::oauth::mcp_resource;
use crate::routes;
use crate::state::AppState;
use crate::test_support::{app_state, migrated_database};

const ORIGIN: &str = "http://localhost:4000";

/// Each test's own admin, with an MCP client of their own. Every test shares one Redis, and a
/// revocation marks its person or client there for a minute, so one shared between tests would
/// keep another test's answers out of the cache.
#[derive(Debug, Clone, Copy)]
struct Person {
    id: Uuid,
    identity: Uuid,
}

impl Person {
    async fn create(connection: &mut diesel_async::AsyncPgConnection) -> Self {
        use diesel_async::RunQueryDsl;

        let person = Self {
            id: Uuid::now_v7(),
            identity: Uuid::now_v7(),
        };
        diesel::sql_query(
            "INSERT INTO users (id, kratos_identity_id, email, name, role, status, approved_at) \
             VALUES ($1, $2, $3, 'Revoker', 'admin', 'active', now())",
        )
        .bind::<diesel::sql_types::Uuid, _>(person.id)
        .bind::<diesel::sql_types::Uuid, _>(person.identity)
        .bind::<diesel::sql_types::Text, _>(person.email())
        .execute(connection)
        .await
        .expect("the person is created");
        person
    }

    fn email(self) -> String {
        format!("{}@example.com", self.id.simple())
    }

    /// The MCP client the fake Hydra issued the person's tokens to.
    fn client_id(self) -> String {
        format!("claude-code-{}", self.id.simple())
    }
}

/// The person's sessions, as Kratos keeps them: each cookie's session and whether it is still
/// active.
#[derive(Default)]
struct KratosSessions {
    by_cookie: HashMap<String, (Session, bool)>,
}

/// Kratos's public and admin routes the API calls, over [`KratosSessions`].
#[derive(Clone)]
struct FakeKratos {
    person: Person,
    sessions: Arc<Mutex<KratosSessions>>,
    whoami_calls: Arc<AtomicUsize>,
    extensions: Arc<AtomicUsize>,
    /// Every `totp_url` an admin patch wrote, oldest first.
    authenticator_urls: Arc<Mutex<Vec<String>>>,
}

impl FakeKratos {
    fn new(person: Person) -> Self {
        Self {
            person,
            sessions: Arc::default(),
            whoami_calls: Arc::default(),
            extensions: Arc::default(),
            authenticator_urls: Arc::default(),
        }
    }

    /// Signs the person in once more, and answers the new session's cookie and id.
    fn sign_in(&self) -> (String, Uuid) {
        self.sign_in_expiring_in(SESSION_LIFESPAN)
    }

    /// Signs the person in with a session that has `remaining` left to live, as one that was
    /// last extended `SESSION_LIFESPAN - remaining` ago.
    fn sign_in_expiring_in(&self, remaining: chrono::Duration) -> (String, Uuid) {
        self.sign_in_with(Utc::now(), remaining)
    }

    /// Signs the person in with a session that last proved who they are `ago`.
    fn signed_in_ago(&self, ago: chrono::Duration) -> String {
        self.sign_in_with(Utc::now() - ago, SESSION_LIFESPAN).0
    }

    fn sign_in_with(
        &self,
        authenticated_at: chrono::DateTime<Utc>,
        remaining: chrono::Duration,
    ) -> (String, Uuid) {
        let cookie = format!("ory_st_{}", Uuid::now_v7().simple());
        let session = Session {
            id: Uuid::now_v7(),
            expires_at: Utc::now() + remaining,
            authenticated_at,
            authenticator_assurance_level: AssuranceLevel::Aal2,
            identity: Identity {
                id: self.person.identity,
                traits: Traits {
                    email: self.person.email(),
                    name: PersonName::Split {
                        first: "Revoker".to_owned(),
                        last: String::new(),
                    },
                },
            },
        };
        let id = session.id;
        self.lock()
            .by_cookie
            .insert(cookie.clone(), (session, true));
        (cookie, id)
    }

    /// Ends a session as Kratos would on its own, such as when a password changes.
    fn end(&self, cookie: &str) {
        if let Some((_, active)) = self.lock().by_cookie.get_mut(cookie) {
            *active = false;
        }
    }

    fn whoami_calls(&self) -> usize {
        self.whoami_calls.load(Ordering::SeqCst)
    }

    fn extensions(&self) -> usize {
        self.extensions.load(Ordering::SeqCst)
    }

    fn authenticator_urls(&self) -> Vec<String> {
        self.authenticator_urls
            .lock()
            .expect("the fake's lock is never poisoned")
            .clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, KratosSessions> {
        self.sessions
            .lock()
            .expect("the fake's lock is never poisoned")
    }

    /// The active session the request's cookie holds.
    fn caller(&self, headers: &HeaderMap) -> Option<(String, Session)> {
        let cookie = session_cookie(headers)?.to_owned();
        let sessions = self.lock();
        let (session, active) = sessions.by_cookie.get(&cookie)?;
        active.then(|| (cookie, session.clone()))
    }

    fn router(&self) -> Router {
        Router::new()
            .route("/sessions/whoami", get(kratos_whoami))
            .route("/self-service/logout/browser", get(kratos_logout_flow))
            .route("/self-service/logout", get(kratos_logout))
            .route("/sessions", delete(kratos_end_others))
            .route("/sessions/{id}", delete(kratos_end_one))
            .route(
                "/admin/identities/{id}/sessions",
                delete(kratos_end_everywhere),
            )
            .route(
                "/admin/identities/{id}",
                get(kratos_identity).patch(kratos_patch_identity),
            )
            .route("/admin/sessions/{id}/extend", patch(kratos_extend))
            .with_state(self.clone())
    }
}

/// Extends a session to a full lifespan from now, as Kratos's admin API does.
async fn kratos_extend(State(kratos): State<FakeKratos>, Path(id): Path<Uuid>) -> Response {
    kratos.extensions.fetch_add(1, Ordering::SeqCst);
    let mut sessions = kratos.lock();
    let found = sessions
        .by_cookie
        .values_mut()
        .find(|(session, _)| session.id == id);
    match found {
        Some((session, _)) => {
            session.expires_at = Utc::now() + SESSION_LIFESPAN;
            axum::Json(session.clone()).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn kratos_whoami(State(kratos): State<FakeKratos>, headers: HeaderMap) -> Response {
    kratos.whoami_calls.fetch_add(1, Ordering::SeqCst);
    match kratos.caller(&headers) {
        Some((_, session)) => axum::Json(session).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

async fn kratos_logout_flow(State(kratos): State<FakeKratos>, headers: HeaderMap) -> Response {
    match kratos.caller(&headers) {
        Some((cookie, _)) => {
            axum::Json(json!({ "logout_token": format!("logout-{cookie}") })).into_response()
        }
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

#[derive(Deserialize)]
struct LogoutQuery {
    token: String,
}

async fn kratos_logout(
    State(kratos): State<FakeKratos>,
    headers: HeaderMap,
    Query(query): Query<LogoutQuery>,
) -> Response {
    let Some((cookie, _)) = kratos.caller(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if query.token != format!("logout-{cookie}") {
        return StatusCode::FORBIDDEN.into_response();
    }
    kratos.end(&cookie);
    (
        StatusCode::NO_CONTENT,
        [(
            header::SET_COOKIE,
            format!("{SESSION_COOKIE}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax"),
        )],
    )
        .into_response()
}

async fn kratos_end_others(State(kratos): State<FakeKratos>, headers: HeaderMap) -> Response {
    let Some((cookie, _)) = kratos.caller(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let mut sessions = kratos.lock();
    let mut count = 0;
    for (other, (_, active)) in &mut sessions.by_cookie {
        if *other != cookie && *active {
            *active = false;
            count += 1;
        }
    }
    axum::Json(json!({ "count": count })).into_response()
}

async fn kratos_end_one(
    State(kratos): State<FakeKratos>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let Some((_, caller)) = kratos.caller(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if id == caller.id {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let mut sessions = kratos.lock();
    let found = sessions
        .by_cookie
        .values_mut()
        .find(|(session, _)| session.id == id && session.identity.id == caller.identity.id);
    match found {
        Some((_, active)) => {
            *active = false;
            StatusCode::NO_CONTENT.into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn kratos_end_everywhere(
    State(kratos): State<FakeKratos>,
    Path(identity): Path<Uuid>,
) -> StatusCode {
    for (session, active) in kratos.lock().by_cookie.values_mut() {
        if session.identity.id == identity {
            *active = false;
        }
    }
    StatusCode::NO_CONTENT
}

/// The person's identity, with a password and an authenticator app.
async fn kratos_identity(Path(id): Path<Uuid>) -> axum::Json<Value> {
    axum::Json(json!({ "id": id, "credentials": { "password": {}, "totp": {} } }))
}

/// Records the `totp_url` a patch writes, as replacing an authenticator app does.
async fn kratos_patch_identity(
    State(kratos): State<FakeKratos>,
    axum::Json(operations): axum::Json<Value>,
) -> axum::Json<Value> {
    let written = operations
        .as_array()
        .into_iter()
        .flatten()
        .find(|operation| operation["path"] == "/credentials/totp/config/totp_url");
    if let Some(url) = written.and_then(|operation| operation["value"].as_str()) {
        kratos
            .authenticator_urls
            .lock()
            .expect("the fake's lock is never poisoned")
            .push(url.to_owned());
    }
    axum::Json(json!({}))
}

/// Hydra's admin routes the API calls: each token's introspection, and whether it is revoked.
#[derive(Clone)]
struct FakeHydra {
    person: Person,
    revoked: Arc<Mutex<HashMap<String, bool>>>,
    introspections: Arc<AtomicUsize>,
}

impl FakeHydra {
    fn new(person: Person) -> Self {
        Self {
            person,
            revoked: Arc::default(),
            introspections: Arc::default(),
        }
    }

    /// A token Hydra issued to the person's client.
    fn issue(&self) -> String {
        let token = format!("ory_at_{}", Uuid::now_v7().simple());
        self.lock().insert(token.clone(), false);
        token
    }

    fn introspections(&self) -> usize {
        self.introspections.load(Ordering::SeqCst)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, bool>> {
        self.revoked
            .lock()
            .expect("the fake's lock is never poisoned")
    }

    /// Revokes every token: the fake issues them all to one person and one client.
    fn revoke_all(&self) {
        for revoked in self.lock().values_mut() {
            *revoked = true;
        }
    }

    fn router(&self) -> Router {
        Router::new()
            .route("/admin/oauth2/introspect", post(hydra_introspect))
            .route(
                "/admin/oauth2/auth/sessions/consent",
                delete(hydra_revoke_consent),
            )
            .route("/admin/clients/{id}", delete(hydra_delete_client))
            .with_state(self.clone())
    }
}

#[derive(Deserialize)]
struct IntrospectForm {
    token: String,
}

async fn hydra_introspect(
    State(hydra): State<FakeHydra>,
    Form(form): Form<IntrospectForm>,
) -> axum::Json<Introspection> {
    hydra.introspections.fetch_add(1, Ordering::SeqCst);
    let active = hydra.lock().get(&form.token) == Some(&false);
    let resource = mcp_resource(&Url::parse(ORIGIN).expect("a static URL parses"));
    axum::Json(Introspection {
        active,
        sub: active.then(|| hydra.person.id.to_string()),
        scope: active.then(|| "workspace:read workspace:write".to_owned()),
        aud: if active { vec![resource] } else { Vec::new() },
        client_id: active.then(|| hydra.person.client_id()),
        exp: active.then(|| Utc::now().timestamp() + 3600),
        token_use: active.then(|| "access_token".to_owned()),
    })
}

async fn hydra_revoke_consent(State(hydra): State<FakeHydra>) -> StatusCode {
    hydra.revoke_all();
    StatusCode::NO_CONTENT
}

async fn hydra_delete_client(State(hydra): State<FakeHydra>) -> StatusCode {
    hydra.revoke_all();
    StatusCode::NO_CONTENT
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

/// The API, its Kratos, and its Hydra, for one person.
struct Stack {
    state: AppState,
    person: Person,
    kratos: FakeKratos,
    hydra: FakeHydra,
}

impl Stack {
    async fn start() -> Self {
        let (database_url, mut connection) = migrated_database().await;
        let person = Person::create(&mut connection).await;
        let mut state = app_state(&database_url).await;
        let (kratos, hydra) = (FakeKratos::new(person), FakeHydra::new(person));
        let kratos_url = serve(kratos.router()).await;
        let hydra_url = serve(hydra.router()).await;
        let http = reqwest::Client::new();
        state.auth.kratos = Kratos::new(http.clone(), kratos_url.clone(), kratos_url);
        state.auth.hydra = Hydra::new(http, hydra_url);
        Self {
            state,
            person,
            kratos,
            hydra,
        }
    }

    /// The same stack with Redis unreachable: a connection that never connects, and gives up on
    /// each command at once rather than retrying.
    fn without_redis(mut self) -> Self {
        let client = redis::Client::open("redis://127.0.0.1:9/").expect("a static URL parses");
        let config = ConnectionManagerConfig::new()
            .set_number_of_retries(0)
            .set_connection_timeout(Some(std::time::Duration::from_millis(200)))
            .set_response_timeout(Some(std::time::Duration::from_millis(200)));
        self.state.redis = ConnectionManager::new_lazy_with_config(client, config)
            .expect("a lazy connection needs no server");
        self
    }

    /// The router's answer to `method path` with `cookie`, and its `Set-Cookie` header if any.
    async fn call(&self, method: Method, path: &str, cookie: &str) -> (StatusCode, Option<String>) {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::COOKIE, format!("{SESSION_COOKIE}={cookie}"))
            .header(header::ORIGIN, ORIGIN)
            .body(Body::empty())
            .expect("a request builds");
        let response = self.send(request).await;
        let set_cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .map(|value| value.to_str().expect("ASCII").to_owned());
        (response.status(), set_cookie)
    }

    /// The router's answer to `POST path` with a JSON `body` and `cookie`, and its JSON.
    async fn post_json(&self, path: &str, cookie: &str, body: &Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(Method::POST)
            .uri(path)
            .header(header::COOKIE, format!("{SESSION_COOKIE}={cookie}"))
            .header(header::ORIGIN, ORIGIN)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("a request builds");
        let response = self.send(request).await;
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the body reads");
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }

    /// Whether the session `cookie` holds reaches the workspace.
    async fn me(&self, cookie: &str) -> StatusCode {
        self.call(Method::GET, "/api/v1/me", cookie).await.0
    }

    /// Lists the MCP server's tools with `token`, answering the status.
    async fn mcp(&self, token: &str) -> StatusCode {
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/mcp")
            .header(header::HOST, "localhost")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .body(Body::from(body.to_string()))
            .expect("a request builds");
        self.send(request).await.status()
    }

    async fn send(&self, request: Request<Body>) -> Response {
        routes::router(&self.state)
            .with_state(self.state.clone())
            .oneshot(request)
            .await
            .expect("the router answers")
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run by scripts/verify-migrations.sh"]
async fn signing_out_refuses_the_session_on_its_next_request() {
    let stack = Stack::start().await;
    let (cookie, _) = stack.kratos.sign_in();
    assert_eq!(stack.me(&cookie).await, StatusCode::OK);
    assert_eq!(stack.me(&cookie).await, StatusCode::OK);
    assert_eq!(
        stack.kratos.whoami_calls(),
        1,
        "the second request was answered from the cache"
    );

    let (status, set_cookie) = stack
        .call(Method::POST, "/api/v1/auth/sign-out", &cookie)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let set_cookie = set_cookie.expect("Kratos's cleared cookie reaches the browser");
    assert!(
        set_cookie.starts_with(&format!("{SESSION_COOKIE}=;")),
        "{set_cookie}"
    );

    assert_eq!(stack.me(&cookie).await, StatusCode::UNAUTHORIZED);
}

/// A cached answer Kratos no longer agrees with, such as one a Redis that was unreachable at a
/// revocation kept, is dropped when the person signs out, though Kratos has no session to end.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run by scripts/verify-migrations.sh"]
async fn signing_out_drops_a_cached_session_kratos_already_ended() {
    let stack = Stack::start().await;
    let (cookie, _) = stack.kratos.sign_in();
    assert_eq!(stack.me(&cookie).await, StatusCode::OK);
    stack.kratos.end(&cookie);
    assert_eq!(
        stack.me(&cookie).await,
        StatusCode::OK,
        "the cache still answers for it"
    );

    let (status, _) = stack
        .call(Method::POST, "/api/v1/auth/sign-out", &cookie)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(stack.me(&cookie).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run by scripts/verify-migrations.sh"]
async fn signing_out_without_a_session_answers_as_done() {
    let stack = Stack::start().await;
    let (status, set_cookie) = stack
        .call(Method::POST, "/api/v1/auth/sign-out", "ory_st_unknown")
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(set_cookie, None);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run by scripts/verify-migrations.sh"]
async fn ending_another_session_refuses_it_on_its_next_request() {
    let stack = Stack::start().await;
    let (laptop, laptop_session) = stack.kratos.sign_in();
    let (phone, phone_session) = stack.kratos.sign_in();
    assert_eq!(stack.me(&laptop).await, StatusCode::OK);
    assert_eq!(stack.me(&phone).await, StatusCode::OK);

    let own = format!("/api/v1/me/sessions/{laptop_session}");
    assert_eq!(
        stack.call(Method::DELETE, &own, &laptop).await.0,
        StatusCode::BAD_REQUEST,
        "the session asking signs out instead"
    );
    let other = format!("/api/v1/me/sessions/{phone_session}");
    assert_eq!(
        stack.call(Method::DELETE, &other, &laptop).await.0,
        StatusCode::NO_CONTENT
    );

    assert_eq!(stack.me(&phone).await, StatusCode::UNAUTHORIZED);
    assert_eq!(stack.me(&laptop).await, StatusCode::OK);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run by scripts/verify-migrations.sh"]
async fn ending_every_other_session_refuses_them_on_their_next_request() {
    let stack = Stack::start().await;
    let (laptop, _) = stack.kratos.sign_in();
    let (phone, _) = stack.kratos.sign_in();
    let (tablet, _) = stack.kratos.sign_in();
    for cookie in [&laptop, &phone, &tablet] {
        assert_eq!(stack.me(cookie).await, StatusCode::OK);
    }

    assert_eq!(
        stack
            .call(Method::DELETE, "/api/v1/me/sessions", &laptop)
            .await
            .0,
        StatusCode::NO_CONTENT
    );

    assert_eq!(stack.me(&phone).await, StatusCode::UNAUTHORIZED);
    assert_eq!(stack.me(&tablet).await, StatusCode::UNAUTHORIZED);
    assert_eq!(stack.me(&laptop).await, StatusCode::OK);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run by scripts/verify-migrations.sh"]
async fn an_admin_signing_someone_out_refuses_their_sessions_and_tokens_at_once() {
    let stack = Stack::start().await;
    let (cookie, _) = stack.kratos.sign_in();
    let token = stack.hydra.issue();
    assert_eq!(stack.me(&cookie).await, StatusCode::OK);
    assert_ne!(stack.mcp(&token).await, StatusCode::UNAUTHORIZED);

    // The person is an admin, and signs themself out everywhere.
    let path = format!("/api/v1/users/{}/revoke-sessions", stack.person.id);
    assert_eq!(
        stack.call(Method::POST, &path, &cookie).await.0,
        StatusCode::NO_CONTENT
    );

    assert_eq!(stack.me(&cookie).await, StatusCode::UNAUTHORIZED);
    assert_eq!(stack.mcp(&token).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run by scripts/verify-migrations.sh"]
async fn a_settings_change_forgets_the_sessions_kratos_ended() {
    let stack = Stack::start().await;
    let (laptop, _) = stack.kratos.sign_in();
    let (phone, _) = stack.kratos.sign_in();
    assert_eq!(stack.me(&phone).await, StatusCode::OK);

    // A new password on the laptop: Kratos signs the phone out, then calls its webhook.
    stack.kratos.end(&phone);
    let request = Request::builder()
        .method(Method::POST)
        .uri("/internal/kratos/settings")
        .header(HOOK_KEY_HEADER, stack.state.auth.hook_key.expose_secret())
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "identity_id": stack.person.identity }).to_string(),
        ))
        .expect("a request builds");
    assert_eq!(stack.send(request).await.status(), StatusCode::NO_CONTENT);

    assert_eq!(stack.me(&phone).await, StatusCode::UNAUTHORIZED);
    assert_eq!(stack.me(&laptop).await, StatusCode::OK);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run by scripts/verify-migrations.sh"]
async fn disconnecting_an_mcp_client_refuses_its_token_on_its_next_request() {
    let stack = Stack::start().await;
    let (cookie, _) = stack.kratos.sign_in();
    let token = stack.hydra.issue();
    assert_ne!(stack.mcp(&token).await, StatusCode::UNAUTHORIZED);
    assert_ne!(stack.mcp(&token).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        stack.hydra.introspections(),
        1,
        "the second call was answered from the cache"
    );

    let path = format!("/api/v1/oauth/grants/{}", stack.person.client_id());
    assert_eq!(
        stack.call(Method::DELETE, &path, &cookie).await.0,
        StatusCode::NO_CONTENT
    );

    assert_eq!(stack.mcp(&token).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run by scripts/verify-migrations.sh"]
async fn deleting_an_mcp_client_refuses_its_tokens_on_their_next_request() {
    let stack = Stack::start().await;
    let (cookie, _) = stack.kratos.sign_in();
    let token = stack.hydra.issue();
    assert_ne!(stack.mcp(&token).await, StatusCode::UNAUTHORIZED);

    let path = format!("/api/v1/oauth/clients/{}", stack.person.client_id());
    assert_eq!(
        stack.call(Method::DELETE, &path, &cookie).await.0,
        StatusCode::NO_CONTENT
    );

    assert_eq!(stack.mcp(&token).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run by scripts/verify-migrations.sh"]
async fn with_redis_down_requests_ask_kratos_and_hydra_and_revoking_still_lands() {
    let stack = Stack::start().await.without_redis();
    let (cookie, _) = stack.kratos.sign_in();
    let token = stack.hydra.issue();

    assert_eq!(stack.me(&cookie).await, StatusCode::OK);
    assert_eq!(stack.me(&cookie).await, StatusCode::OK);
    assert_eq!(stack.kratos.whoami_calls(), 2, "every request asked Kratos");
    assert_ne!(stack.mcp(&token).await, StatusCode::UNAUTHORIZED);

    let path = format!("/api/v1/oauth/grants/{}", stack.person.client_id());
    assert_eq!(
        stack.call(Method::DELETE, &path, &cookie).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(stack.mcp(&token).await, StatusCode::UNAUTHORIZED);

    let (status, _) = stack
        .call(Method::POST, "/api/v1/auth/sign-out", &cookie)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(stack.me(&cookie).await, StatusCode::UNAUTHORIZED);
}

/// #190: the long cache never holds back a rolling extension. The first request after a session
/// is due extends it and drops the cached answer, so the next one reads the new expiry from
/// Kratos, and the one after that is answered from the cache, not extended again.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run by scripts/verify-migrations.sh"]
async fn a_session_in_use_rolls_though_its_answer_is_cached() {
    let stack = Stack::start().await;
    // Last extended two hours ago: past the hour after which a request extends it.
    let (cookie, _) = stack
        .kratos
        .sign_in_expiring_in(SESSION_LIFESPAN - chrono::Duration::hours(2));

    assert_eq!(stack.me(&cookie).await, StatusCode::OK);
    assert_eq!(stack.kratos.extensions(), 1);
    assert_eq!(stack.me(&cookie).await, StatusCode::OK);
    assert_eq!(stack.me(&cookie).await, StatusCode::OK);

    assert_eq!(
        stack.kratos.extensions(),
        1,
        "extended once, when it was due"
    );
    assert_eq!(
        stack.kratos.whoami_calls(),
        2,
        "asked again once after the extension, then answered from the cache"
    );
}

/// The code an authenticator app holding `secret_key` shows now.
fn current_code(secret_key: &str) -> String {
    let secret = totp_rs::Secret::try_from_base32(secret_key).expect("the key is base32");
    totp_rs::Builder::new()
        .with_secret(secret)
        .build()
        .expect("a 160-bit secret builds")
        .generate_current()
        .to_string()
}

/// #165: moving to a new authenticator app swaps the secret in Kratos through its admin API,
/// which fires no settings webhook, so the move forgets the person's cached sessions itself and
/// every other session asks Kratos again on its next request.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run by scripts/verify-migrations.sh"]
async fn moving_to_a_new_authenticator_app_forgets_the_cached_sessions() {
    let stack = Stack::start().await;
    let (laptop, _) = stack.kratos.sign_in();
    let (phone, _) = stack.kratos.sign_in();
    assert_eq!(stack.me(&phone).await, StatusCode::OK);
    assert_eq!(stack.me(&phone).await, StatusCode::OK);

    let (status, started) = stack
        .post_json("/api/v1/me/authenticator-replacement", &laptop, &json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let secret_key = started["secretKey"].as_str().expect("a key to type");
    assert!(
        stack.kratos.authenticator_urls().is_empty(),
        "the old app is still the one in use"
    );

    let (status, refused) = stack
        .post_json(
            "/api/v1/me/authenticator-replacement/confirm",
            &laptop,
            &json!({ "code": "000000", "nickname": "Bitwarden" }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(refused["code"], "authenticator_code_invalid");
    assert!(
        stack.kratos.authenticator_urls().is_empty(),
        "a wrong code changes nothing"
    );

    let (status, _) = stack
        .post_json(
            "/api/v1/me/authenticator-replacement/confirm",
            &laptop,
            &json!({ "code": current_code(secret_key), "nickname": "Bitwarden" }),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let written = stack.kratos.authenticator_urls();
    assert_eq!(written.len(), 1);
    assert!(
        written[0].contains(&format!("secret={secret_key}")),
        "{}",
        written[0]
    );

    let asked_after_move = stack.kratos.whoami_calls();
    assert_eq!(stack.me(&phone).await, StatusCode::OK);
    assert_eq!(
        stack.kratos.whoami_calls(),
        asked_after_move + 1,
        "the phone's cached answer was forgotten, so it asked Kratos again"
    );

    let (status, _) = stack
        .post_json(
            "/api/v1/me/authenticator-replacement/confirm",
            &laptop,
            &json!({ "code": current_code(secret_key), "nickname": "Bitwarden" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a finished move cannot be confirmed twice"
    );
}

/// #165: a move changes how the person signs in, so it asks for a recent sign-in as Kratos does,
/// read from Kratos rather than the cache.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run by scripts/verify-migrations.sh"]
async fn moving_to_a_new_authenticator_app_needs_a_recent_sign_in() {
    let stack = Stack::start().await;
    let stale = stack.kratos.signed_in_ago(chrono::Duration::minutes(20));
    let (status, refused) = stack
        .post_json("/api/v1/me/authenticator-replacement", &stale, &json!({}))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(refused["code"], "recent_sign_in_required");

    let fresh = stack.kratos.signed_in_ago(chrono::Duration::minutes(1));
    let (status, _) = stack
        .post_json("/api/v1/me/authenticator-replacement", &fresh, &json!({}))
        .await;
    assert_eq!(status, StatusCode::OK);
}
