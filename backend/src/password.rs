//! Sign-in method discovery, OPAQUE password auth, and TOTP verification.
//!
//! These are the sign-in handlers for the password path, complementing the
//! passkey ceremony in [`crate::webauthn`]. The OPAQUE and TOTP cryptography live
//! in [`crate::opaque`] and [`crate::totp`]; this module is the HTTP shell around
//! them: it carries in-flight state in short-lived cookies, enforces per-identifier
//! rate limiting, and records every attempt in the login history.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::cookies;
use crate::login_events::{self, ClientInfo};
use crate::opaque;
use crate::session;
use crate::state::AppState;
use crate::totp;
use crate::users;

/// Short-lived cookie carrying the in-flight OPAQUE login state.
const PASSWORD_COOKIE: &str = "elysium_password";

/// Short-lived cookie marking a password login awaiting its TOTP second factor.
const TOTP_PENDING_COOKIE: &str = "elysium_totp_pending";

/// Failed password attempts per identifier before the endpoint rate-limits.
///
/// A deliberately small window: OPAQUE resists offline attack, so this only has
/// to blunt online guessing. Tuned with `RATE_LIMIT_WINDOW_MINUTES`.
const MAX_RECENT_FAILURES: i64 = 5;

/// Window over which `MAX_RECENT_FAILURES` is counted, in minutes.
const RATE_LIMIT_WINDOW_MINUTES: i64 = 15;

/// Normalizes an identifier to its canonical form (trimmed, lowercased email).
///
/// The same normalization is used at registration, so the OPAQUE identifier and
/// the user lookup agree between enrollment and sign-in.
pub fn normalize(identifier: &str) -> String {
    identifier.trim().to_lowercase()
}

/// Body of the method-discovery request.
#[derive(Deserialize)]
pub struct MethodsRequest {
    identifier: String,
}

/// `POST /api/v1/auth/methods`: which sign-in methods this instance permits.
///
/// Returns the methods allowed by the relevant org policy, never which
/// credentials the account holds, so the response cannot be used to probe which
/// emails exist. An unknown identifier gets the default org's policy.
pub async fn methods(State(state): State<AppState>, Json(body): Json<MethodsRequest>) -> Response {
    match allowed_methods(&state, &normalize(&body.identifier)).await {
        Ok(methods) => Json(json!({ "methods": methods })).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "method discovery failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// Resolves the permitted method list for a profile.
///
/// A profile can belong to many orgs, so a method is offered if ANY of its orgs'
/// policies allow it (union). An unknown identifier, or a profile with no orgs
/// yet, gets the buildable defaults, so the response is the same shape either way
/// and reveals nothing about which accounts exist. Sign-in is profile-global;
/// per-org enforcement (e.g. an org that mandates TOTP) is applied after sign-in.
async fn allowed_methods(state: &AppState, identifier: &str) -> sqlx::Result<Vec<&'static str>> {
    // `bool_or` over the profile's org policies; all NULL when no policy matched.
    let (passkeys, passwords, magic_links) = sqlx::query_as::<
        _,
        (Option<bool>, Option<bool>, Option<bool>),
    >(
        "SELECT bool_or(p.allow_passkeys), bool_or(p.allow_passwords), bool_or(p.allow_magic_links)
               FROM users u
               JOIN org_members m ON m.user_id = u.id
               JOIN org_auth_policies p ON p.org_id = m.org_id
              WHERE lower(u.email) = $1",
    )
    .bind(identifier)
    .fetch_one(&state.pool)
    .await?;

    // No matching policy (unknown identifier or a profile not yet in an org): offer
    // the buildable defaults.
    if passkeys.is_none() && passwords.is_none() && magic_links.is_none() {
        return Ok(vec!["passkey", "password"]);
    }

    let mut methods = Vec::new();
    if passkeys.unwrap_or(false) {
        methods.push("passkey");
    }
    if passwords.unwrap_or(false) {
        methods.push("password");
    }
    if magic_links.unwrap_or(false) {
        methods.push("magic_link");
    }
    Ok(methods)
}

/// In-flight OPAQUE login state carried between the begin and finish requests.
#[derive(Serialize, Deserialize)]
struct PendingLogin {
    /// The user, or `None` for the indistinguishable unknown-user (dummy) path.
    user_id: Option<Uuid>,
    /// The presented identifier, kept for the login-history record.
    identifier: String,
    /// The serialized `ServerLogin` state, base64 for a compact cookie.
    state: String,
}

/// Body of the password authenticate-begin request.
#[derive(Deserialize)]
pub struct PasswordBegin {
    identifier: String,
    ke1: String,
}

/// `POST /api/v1/auth/password/authenticate/begin`: start an OPAQUE login.
///
/// Always returns a KE2 message, even for an unknown identifier (a dummy
/// response), so the endpoint never reveals whether an account exists.
pub async fn authenticate_begin(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<PasswordBegin>,
) -> Response {
    let identifier = normalize(&body.identifier);

    match too_many_recent_failures(&state, &identifier).await {
        Ok(true) => return StatusCode::TOO_MANY_REQUESTS.into_response(),
        Ok(false) => {}
        Err(error) => {
            tracing::error!(error = %error, "rate-limit check failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    // Look up the user and their record; an unknown user yields `None`, which
    // opaque::login_start turns into an indistinguishable dummy response.
    let user = match users::find_by_email(&state.pool, &identifier).await {
        Ok(user) => user,
        Err(error) => {
            tracing::error!(error = %error, "user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    let record = match &user {
        Some(user) => match users::password_record(&state.pool, user.id).await {
            Ok(record) => record,
            Err(error) => {
                tracing::error!(error = %error, "password record lookup failed");
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
        },
        None => None,
    };

    let start = match opaque::login_start(
        &state.opaque_setup,
        record.as_deref(),
        &body.ke1,
        identifier.as_bytes(),
    ) {
        Ok(start) => start,
        Err(error) => {
            tracing::debug!(error = %error, "opaque login start failed");
            return (StatusCode::BAD_REQUEST, "invalid login request").into_response();
        }
    };

    let pending = PendingLogin {
        user_id: user.map(|user| user.id),
        identifier,
        state: STANDARD.encode(&start.state),
    };
    let cookie = cookies::build(
        PASSWORD_COOKIE,
        serde_json::to_string(&pending).expect("pending login serializes"),
    );
    (jar.add(cookie), Json(json!({ "ke2": start.ke2 }))).into_response()
}

/// Body of the password authenticate-finish request.
#[derive(Deserialize)]
pub struct PasswordFinish {
    ke3: String,
}

/// `POST /api/v1/auth/password/authenticate/finish`: verify KE3, then sign in.
///
/// On success the response is `204` with a session cookie, unless the account has
/// a TOTP factor, in which case it is `401 { "reason": "totp_required" }` and a
/// pending-TOTP cookie is set for [`verify_totp`] to complete.
pub async fn authenticate_finish(
    State(state): State<AppState>,
    jar: CookieJar,
    client: ClientInfo,
    Json(body): Json<PasswordFinish>,
) -> Response {
    let Some(pending) = read_pending(&jar) else {
        return (StatusCode::BAD_REQUEST, "no login in progress").into_response();
    };
    let jar = jar.remove(cookies::clearing(PASSWORD_COOKIE));

    let Ok(state_bytes) = STANDARD.decode(&pending.state) else {
        return StatusCode::BAD_REQUEST.into_response();
    };

    // A wrong password (or the dummy unknown-user exchange) fails verification
    // here; both look identical to the client.
    let verified = opaque::login_finish(&state_bytes, &body.ke3).is_ok();
    if !verified || pending.user_id.is_none() {
        login_events::record(
            &state.pool,
            pending.user_id,
            &pending.identifier,
            login_events::Method::Password,
            login_events::Outcome::Failure,
            &client,
        )
        .await;
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let user_id = pending.user_id.expect("checked to be Some above");
    login_events::record(
        &state.pool,
        Some(user_id),
        &pending.identifier,
        login_events::Method::Password,
        login_events::Outcome::Success,
        &client,
    )
    .await;

    // A password success is a complete sign-in only when no second factor is
    // enrolled; otherwise hand off to the TOTP step.
    match users::totp_secret(&state.pool, user_id).await {
        Ok(Some(_)) => {
            let cookie = cookies::build(TOTP_PENDING_COOKIE, user_id.to_string());
            (
                StatusCode::UNAUTHORIZED,
                jar.add(cookie),
                Json(json!({ "reason": "totp_required" })),
            )
                .into_response()
        }
        Ok(None) => session::establish(&state.pool, jar, user_id).await,
        Err(error) => {
            tracing::error!(error = %error, "totp lookup failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Body of the TOTP verification request.
#[derive(Deserialize)]
pub struct TotpVerify {
    code: String,
}

/// `POST /api/v1/auth/totp/verify`: complete a password login's second factor.
pub async fn verify_totp(
    State(state): State<AppState>,
    jar: CookieJar,
    client: ClientInfo,
    Json(body): Json<TotpVerify>,
) -> Response {
    let Some(user_id) = jar
        .get(TOTP_PENDING_COOKIE)
        .and_then(|entry| Uuid::parse_str(entry.value()).ok())
    else {
        return (StatusCode::BAD_REQUEST, "no second factor in progress").into_response();
    };

    let secret = match users::totp_secret(&state.pool, user_id).await {
        Ok(Some(secret)) => secret,
        Ok(None) => return StatusCode::BAD_REQUEST.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "totp lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let accepted = totp::verify(&secret, &body.code).unwrap_or(false);
    let outcome = if accepted {
        login_events::Outcome::Success
    } else {
        login_events::Outcome::Failure
    };
    login_events::record(
        &state.pool,
        Some(user_id),
        "",
        login_events::Method::Totp,
        outcome,
        &client,
    )
    .await;

    if !accepted {
        // Keep the pending cookie so the user can retry the code; the failure is
        // recorded and the password rate limit still governs abuse.
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let jar = jar.remove(cookies::clearing(TOTP_PENDING_COOKIE));
    session::establish(&state.pool, jar, user_id).await
}

/// Whether `identifier` has exceeded the recent-failure threshold.
///
/// # Errors
/// Returns an error if the count query fails.
async fn too_many_recent_failures(state: &AppState, identifier: &str) -> sqlx::Result<bool> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM login_events
          WHERE lower(email) = $1
            AND method = 'password'
            AND outcome = 'failure'
            AND created_at > now() - make_interval(mins => $2)",
    )
    .bind(identifier)
    .bind(RATE_LIMIT_WINDOW_MINUTES as i32)
    .fetch_one(&state.pool)
    .await?;
    Ok(count >= MAX_RECENT_FAILURES)
}

/// Reads the in-flight OPAQUE login state from the cookie, if present and valid.
fn read_pending(jar: &CookieJar) -> Option<PendingLogin> {
    jar.get(PASSWORD_COOKIE)
        .and_then(|entry| serde_json::from_str(entry.value()).ok())
}
