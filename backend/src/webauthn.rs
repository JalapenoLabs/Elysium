//! WebAuthn Relying Party: the API runs the passkey ceremony itself.
//!
//! Per `docs/auth-model.md`, the browser never contacts Keycloak. The SPA calls
//! these endpoints and drives the native `navigator.credentials` ceremony; this
//! module (the Relying Party) issues challenges and verifies assertions, stores
//! passkeys in Postgres, and establishes the session cookie on success.
//!
//! The WebAuthn interchange is standardized JSON (what `navigator.credentials`
//! consumes and produces), so these endpoints are JSON rather than protobuf. The
//! in-flight ceremony state rides a short-lived HttpOnly cookie.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use base64::Engine;
use eyre::WrapErr;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use webauthn_rs::prelude::{
    Passkey, PasskeyAuthentication, PasskeyRegistration, PublicKeyCredential,
    RegisterPublicKeyCredential, Url, WebauthnBuilder,
};
use webauthn_rs::Webauthn;

use crate::session::{self, SESSION_COOKIE};
use crate::state::AppState;
use crate::users;

/// Human-facing Relying Party name shown by some authenticators.
const RP_NAME: &str = "Elysium";

/// Short-lived cookie carrying the in-flight ceremony state between begin/finish.
const CEREMONY_COOKIE: &str = "elysium_webauthn";

/// Builds the Relying Party from the derived deployment config.
///
/// The RP id is the app's registrable domain and the RP origin is the app origin.
/// Because the SPA and API share one origin (the SPA proxies `/api` to the API),
/// the origin the API validates is exactly the origin the browser signs against.
///
/// # Errors
/// Returns an error if the origin cannot be parsed or the RP cannot be built.
pub fn build(rp_id: &str, rp_origin: &str) -> eyre::Result<Arc<Webauthn>> {
    let origin = Url::parse(rp_origin).wrap_err("RP origin is not a valid URL")?;
    let webauthn = WebauthnBuilder::new(rp_id, &origin)
        .wrap_err("invalid WebAuthn Relying Party id or origin")?
        .rp_name(RP_NAME)
        .build()
        .wrap_err("failed to build the WebAuthn Relying Party")?;
    Ok(Arc::new(webauthn))
}

/// In-flight ceremony state, stashed in the cookie between begin and finish.
#[derive(Serialize, Deserialize)]
enum Ceremony {
    Register {
        user_id: Uuid,
        state: PasskeyRegistration,
    },
    Authenticate {
        user_id: Uuid,
        state: PasskeyAuthentication,
    },
}

/// `GET /api/v1/auth/context`: is the instance claimed yet? Drives first-run.
pub async fn context(State(state): State<AppState>) -> Response {
    match users::any_exist(&state.pool).await {
        Ok(claimed) => Json(json!({ "claimed": claimed })).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "context lookup failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// Body of the bootstrap request: the root user's identity and organization.
#[derive(Deserialize)]
pub struct BootstrapBegin {
    email: String,
    display_name: String,
    org_name: String,
}

/// `POST /api/v1/auth/bootstrap/begin`: claim an unclaimed instance and start the
/// root user's first passkey registration.
pub async fn bootstrap_begin(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<BootstrapBegin>,
) -> Response {
    match users::any_exist(&state.pool).await {
        Ok(true) => return (StatusCode::CONFLICT, "instance already claimed").into_response(),
        Ok(false) => {}
        Err(error) => {
            tracing::error!(error = %error, "bootstrap claim check failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    let user = match users::create_root(
        &state.pool,
        &body.email,
        &body.display_name,
        &body.org_name,
    )
    .await
    {
        Ok(user) => user,
        Err(error) => {
            tracing::error!(error = %error, "failed to create the root user");
            return (StatusCode::INTERNAL_SERVER_ERROR, "could not create user").into_response();
        }
    };

    let display_name = if body.display_name.is_empty() {
        &body.email
    } else {
        &body.display_name
    };
    let (challenge, registration) =
        match state
            .webauthn
            .start_passkey_registration(user.id, &user.email, display_name, None)
        {
            Ok(pair) => pair,
            Err(error) => {
                tracing::error!(error = %error, "start_passkey_registration failed");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };

    let ceremony = Ceremony::Register {
        user_id: user.id,
        state: registration,
    };
    (jar.add(ceremony_cookie(&ceremony)), Json(challenge)).into_response()
}

/// `POST /api/v1/auth/bootstrap/finish`: verify the first passkey and sign in.
pub async fn bootstrap_finish(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(credential): Json<RegisterPublicKeyCredential>,
) -> Response {
    let Some(Ceremony::Register {
        user_id,
        state: registration,
    }) = read_ceremony(&jar)
    else {
        return (StatusCode::BAD_REQUEST, "no registration in progress").into_response();
    };

    let passkey = match state
        .webauthn
        .finish_passkey_registration(&credential, &registration)
    {
        Ok(passkey) => passkey,
        Err(error) => {
            tracing::debug!(error = %error, "passkey registration verification failed");
            return (StatusCode::BAD_REQUEST, "registration failed").into_response();
        }
    };

    if let Err(error) = store_credential(&state.pool, user_id, &passkey).await {
        tracing::error!(error = %error, "failed to store passkey");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    establish_session(&state.pool, clear_ceremony(jar), user_id).await
}

/// Body of the authentication request: the account to sign in.
#[derive(Deserialize)]
pub struct AuthenticateBegin {
    email: String,
}

/// `POST /api/v1/auth/passkey/authenticate/begin`: start a passkey sign-in.
pub async fn authenticate_begin(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<AuthenticateBegin>,
) -> Response {
    // Look up the user and their passkeys. A missing user or no credentials is
    // reported as a generic 401 so the endpoint does not reveal which emails exist.
    let user = match users::find_by_email(&state.pool, &body.email).await {
        Ok(Some(user)) => user,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let passkeys = match credentials_for(&state.pool, user.id).await {
        Ok(passkeys) if !passkeys.is_empty() => passkeys,
        Ok(_) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "credential lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let (challenge, authentication) = match state.webauthn.start_passkey_authentication(&passkeys) {
        Ok(pair) => pair,
        Err(error) => {
            tracing::error!(error = %error, "start_passkey_authentication failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let ceremony = Ceremony::Authenticate {
        user_id: user.id,
        state: authentication,
    };
    (jar.add(ceremony_cookie(&ceremony)), Json(challenge)).into_response()
}

/// `POST /api/v1/auth/passkey/authenticate/finish`: verify the assertion, sign in.
pub async fn authenticate_finish(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(credential): Json<PublicKeyCredential>,
) -> Response {
    let Some(Ceremony::Authenticate {
        user_id,
        state: authentication,
    }) = read_ceremony(&jar)
    else {
        return (StatusCode::BAD_REQUEST, "no authentication in progress").into_response();
    };

    let result = match state
        .webauthn
        .finish_passkey_authentication(&credential, &authentication)
    {
        Ok(result) => result,
        Err(error) => {
            tracing::debug!(error = %error, "passkey authentication verification failed");
            return StatusCode::UNAUTHORIZED.into_response();
        }
    };

    // Persist the updated signature counter to detect cloned authenticators.
    if result.needs_update() {
        if let Err(error) = update_credential(&state.pool, &result).await {
            tracing::error!(error = %error, "failed to update passkey counter");
        }
    }

    establish_session(&state.pool, clear_ceremony(jar), user_id).await
}

/// `POST /api/v1/auth/logout`: end the session and clear the cookie.
pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> Response {
    if let Some(id) = jar
        .get(SESSION_COOKIE)
        .and_then(|entry| Uuid::parse_str(entry.value()).ok())
    {
        if let Err(error) = session::delete(&state.pool, id).await {
            tracing::error!(error = %error, "failed to delete session");
        }
    }
    (
        jar.remove(Cookie::from(SESSION_COOKIE)),
        StatusCode::NO_CONTENT,
    )
        .into_response()
}

/// Creates a session for the user and returns a 204 with the session cookie set.
async fn establish_session(pool: &PgPool, jar: CookieJar, user_id: Uuid) -> Response {
    match session::create(pool, user_id).await {
        Ok(session_id) => {
            let cookie = Cookie::build((SESSION_COOKIE, session_id.to_string()))
                .http_only(true)
                .secure(true)
                .same_site(SameSite::Lax)
                .path("/")
                .build();
            (jar.add(cookie), StatusCode::NO_CONTENT).into_response()
        }
        Err(error) => {
            tracing::error!(error = %error, "failed to create session");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Persists a newly-registered passkey for a user.
async fn store_credential(pool: &PgPool, user_id: Uuid, passkey: &Passkey) -> eyre::Result<()> {
    let credential_id = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(passkey.cred_id());
    let serialized = serde_json::to_value(passkey).wrap_err("failed to serialize passkey")?;

    sqlx::query(
        "INSERT INTO webauthn_credentials (user_id, credential_id, passkey) VALUES ($1, $2, $3)",
    )
    .bind(user_id)
    .bind(credential_id)
    .bind(serialized)
    .execute(pool)
    .await
    .wrap_err("failed to store passkey")?;

    Ok(())
}

/// Loads a user's registered passkeys.
async fn credentials_for(pool: &PgPool, user_id: Uuid) -> eyre::Result<Vec<Passkey>> {
    let rows = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT passkey FROM webauthn_credentials WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .wrap_err("failed to load passkeys")?;

    let mut passkeys = Vec::with_capacity(rows.len());
    for row in rows {
        passkeys.push(serde_json::from_value(row).wrap_err("failed to deserialize passkey")?);
    }
    Ok(passkeys)
}

/// Applies the post-authentication signature-counter update to the stored passkey.
async fn update_credential(
    pool: &PgPool,
    result: &webauthn_rs::prelude::AuthenticationResult,
) -> eyre::Result<()> {
    let credential_id = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(result.cred_id());

    let stored = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT passkey FROM webauthn_credentials WHERE credential_id = $1",
    )
    .bind(&credential_id)
    .fetch_optional(pool)
    .await
    .wrap_err("failed to load passkey for update")?;

    let Some(stored) = stored else {
        return Ok(());
    };
    let mut passkey: Passkey =
        serde_json::from_value(stored).wrap_err("failed to deserialize passkey for update")?;
    passkey.update_credential(result);
    let updated = serde_json::to_value(&passkey).wrap_err("failed to serialize updated passkey")?;

    sqlx::query(
        "UPDATE webauthn_credentials SET passkey = $1, last_used_at = now() WHERE credential_id = $2",
    )
    .bind(updated)
    .bind(&credential_id)
    .execute(pool)
    .await
    .wrap_err("failed to update passkey")?;

    Ok(())
}

/// Builds the short-lived HttpOnly cookie holding the in-flight ceremony state.
fn ceremony_cookie(ceremony: &Ceremony) -> Cookie<'static> {
    let value = serde_json::to_string(ceremony).expect("ceremony state serializes");
    Cookie::build((CEREMONY_COOKIE, value))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/")
        .build()
}

/// Reads the in-flight ceremony state from the cookie, if present and valid.
fn read_ceremony(jar: &CookieJar) -> Option<Ceremony> {
    jar.get(CEREMONY_COOKIE)
        .and_then(|entry| serde_json::from_str(entry.value()).ok())
}

/// Clears the ceremony cookie.
fn clear_ceremony(jar: CookieJar) -> CookieJar {
    jar.remove(Cookie::from(CEREMONY_COOKIE))
}
