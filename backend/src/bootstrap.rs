//! First-run instance claim: the atomic two-step bootstrap.
//!
//! A fresh instance has no users; the first (root) user claims it here. The flow
//! is method-aware (passkey or OPAQUE password + TOTP) and, crucially, atomic:
//! `begin` persists nothing, and `finish` writes the org, its auth policy, the
//! user, their credential, their recovery codes, and a session in a single
//! transaction only after the chosen credential verifies. Abandoning `begin`
//! therefore leaves the instance unclaimed rather than claimed-with-no-way-in.
//!
//! The credential ceremony itself is shared with invite acceptance and
//! add-credential via [`crate::enrollment`]; this module only adds the org, its
//! policy, and the root user around it.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::crypto::Crypto;
use crate::enrollment::{self, Credential, FinishBody, PendingCredential};
use crate::login_events::{self, ClientInfo};
use crate::session::{self, SESSION_COOKIE};
use crate::state::AppState;
use crate::{cookies, recovery, users};

/// Short-lived cookie carrying the in-flight bootstrap state between begin/finish.
const BOOTSTRAP_COOKIE: &str = "elysium_bootstrap";

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

/// The full in-flight bootstrap, serialized into the cookie.
#[derive(Serialize, Deserialize)]
struct Pending {
    /// The candidate user id, also the WebAuthn user handle for the passkey path.
    user_id: Uuid,
    org_name: String,
    email: String,
    given_name: String,
    family_name: String,
    display_name: String,
    credential: PendingCredential,
}

/// Body of the bootstrap-begin request.
#[derive(Deserialize)]
pub struct BootstrapBegin {
    org_name: String,
    email: String,
    #[serde(default)]
    given_name: String,
    #[serde(default)]
    family_name: String,
    /// What the UI renders; falls back to `given_name`, then `email`, when blank.
    #[serde(default)]
    display_name: String,
    /// The chosen auth method: `"passkey"` or `"password"`.
    method: String,
    /// OPAQUE registration request (base64); required for the password method.
    opaque_registration_request: Option<String>,
}

/// `POST /api/v1/auth/bootstrap/begin`: start claiming an unclaimed instance.
///
/// Persists nothing. Returns the enrollment payload for the chosen method (a
/// passkey challenge, or the OPAQUE registration response + a TOTP provisioning
/// URI) and stashes the in-flight state in a short-lived cookie.
pub async fn begin(
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

    let user_id = Uuid::new_v4();
    // The rendered name defaults to the given name, then the email, when blank.
    let display_name = match (body.display_name.trim(), body.given_name.trim()) {
        ("", "") => body.email.clone(),
        ("", given) => given.to_string(),
        (chosen, _) => chosen.to_string(),
    };

    let (credential, payload) = match enrollment::begin(
        &state,
        user_id,
        &body.email,
        &display_name,
        &body.method,
        body.opaque_registration_request.as_deref(),
    ) {
        Ok(pair) => pair,
        Err(response) => return response,
    };

    let pending = Pending {
        user_id,
        org_name: body.org_name,
        email: body.email,
        given_name: body.given_name.trim().to_string(),
        family_name: body.family_name.trim().to_string(),
        display_name,
        credential,
    };
    let cookie = cookies::build(
        BOOTSTRAP_COOKIE,
        serde_json::to_string(&pending).expect("pending bootstrap serializes"),
    );
    (jar.add(cookie), Json(payload)).into_response()
}

/// `POST /api/v1/auth/bootstrap/finish`: verify the credential and claim atomically.
///
/// On success returns `200` with the one-time recovery codes to display, and sets
/// the session cookie. Nothing was persisted before this point, so a failed or
/// abandoned begin leaves no trace.
pub async fn finish(
    State(state): State<AppState>,
    jar: CookieJar,
    client: ClientInfo,
    Json(body): Json<FinishBody>,
) -> Response {
    let Some(pending) = read_pending(&jar) else {
        return (StatusCode::BAD_REQUEST, "no bootstrap in progress").into_response();
    };
    let jar = jar.remove(cookies::clearing(BOOTSTRAP_COOKIE));

    // Guard against a racing claim between begin and finish.
    match users::any_exist(&state.pool).await {
        Ok(true) => return (StatusCode::CONFLICT, "instance already claimed").into_response(),
        Ok(false) => {}
        Err(error) => {
            tracing::error!(error = %error, "bootstrap claim check failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    let credential = match enrollment::verify(&state, &pending.credential, &body) {
        Ok(credential) => credential,
        Err(response) => return response,
    };
    let method = credential.method();

    let codes = recovery::generate();
    let session_id = match claim(
        &state.pool,
        &state.crypto,
        &client,
        &pending,
        &credential,
        &codes,
    )
    .await
    {
        Ok(session_id) => session_id,
        Err(error) => {
            tracing::error!(error = %error, "failed to claim instance");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    login_events::record(
        &state.pool,
        Some(pending.user_id),
        &pending.email,
        method,
        login_events::Outcome::Success,
        &client,
    )
    .await;

    let cookie = cookies::build(SESSION_COOKIE, session_id.to_string());
    (jar.add(cookie), Json(json!({ "recovery_codes": codes }))).into_response()
}

/// Creates the org, its policy, the user, credential, recovery codes, and session.
///
/// One transaction: either the instance is fully claimed or nothing changes. The
/// policy is seeded to the chosen method, so an org that claimed with a password
/// requires TOTP from the start.
///
/// # Errors
/// Returns an error if any insert fails (e.g. a raced claim violating uniqueness).
async fn claim(
    pool: &PgPool,
    crypto: &Crypto,
    client: &ClientInfo,
    pending: &Pending,
    credential: &Credential,
    recovery_codes: &[String],
) -> sqlx::Result<Uuid> {
    let password = matches!(credential, Credential::Password { .. });

    let mut tx = pool.begin().await?;

    let org_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO orgs (name, slug) VALUES ($1, 'root') RETURNING id",
    )
    .bind(&pending.org_name)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO org_auth_policies (org_id, allow_passkeys, allow_passwords, require_totp)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(org_id)
    .bind(!password)
    .bind(password)
    .bind(password)
    .execute(&mut *tx)
    .await?;

    // The user id is fixed from begin so the passkey's WebAuthn user handle
    // matches; the subject is a separate opaque identifier we own. The profile is
    // org-independent; the owner membership is added below.
    sqlx::query(
        "INSERT INTO users (id, subject, email, given_name, family_name, display_name)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(pending.user_id)
    .bind(Uuid::new_v4().to_string())
    .bind(&pending.email)
    .bind(&pending.given_name)
    .bind(&pending.family_name)
    .bind(&pending.display_name)
    .execute(&mut *tx)
    .await?;

    users::add_membership(&mut tx, pending.user_id, org_id, "owner").await?;
    enrollment::insert(&mut tx, crypto, pending.user_id, credential).await?;
    recovery::store(&mut tx, pending.user_id, recovery_codes).await?;

    let session_id = session::create_on(&mut tx, pending.user_id, Some(org_id), client).await?;

    tx.commit().await?;
    Ok(session_id)
}

/// Reads the in-flight bootstrap state from the cookie, if present and valid.
fn read_pending(jar: &CookieJar) -> Option<Pending> {
    jar.get(BOOTSTRAP_COOKIE)
        .and_then(|entry| serde_json::from_str(entry.value()).ok())
}
