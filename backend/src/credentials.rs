//! Authenticated add-credential: enroll a passkey or set/change a password.
//!
//! Session-gated enrollment for an already-signed-in user, reusing
//! [`crate::enrollment`]. It is the enrollment half of the Security Center and,
//! crucially, the way a user who signed in with a recovery code re-establishes a
//! real credential after losing their device. Removing or listing credentials is
//! a later Security Center step; this covers adding one.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enrollment::{self, FinishBody, PendingCredential};
use crate::session::SessionUser;
use crate::state::AppState;
use crate::{cookies, users};

/// Short-lived cookie carrying the in-flight add-credential state.
const CREDENTIAL_COOKIE: &str = "elysium_add_credential";

/// The in-flight add-credential enrollment, serialized into the cookie.
#[derive(Serialize, Deserialize)]
struct Pending {
    user_id: Uuid,
    credential: PendingCredential,
}

/// Body of the add-credential begin request.
#[derive(Deserialize)]
pub struct EnrollBegin {
    /// `"passkey"` or `"password"`.
    method: String,
    opaque_registration_request: Option<String>,
}

/// `POST /api/v1/credentials/enroll/begin`: start adding a credential.
///
/// Enrolls against the authenticated user's own identity (their id is the passkey
/// user handle, their email the OPAQUE identifier), so the new credential binds to
/// their existing account.
pub async fn enroll_begin(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    jar: CookieJar,
    Json(body): Json<EnrollBegin>,
) -> Response {
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let display_name = if principal.username.is_empty() {
        &principal.email
    } else {
        &principal.username
    };
    let (credential, payload) = match enrollment::begin(
        &state,
        user_id,
        &principal.email,
        display_name,
        &body.method,
        body.opaque_registration_request.as_deref(),
    ) {
        Ok(pair) => pair,
        Err(response) => return response,
    };

    let pending = Pending {
        user_id,
        credential,
    };
    let cookie = cookies::build(
        CREDENTIAL_COOKIE,
        serde_json::to_string(&pending).expect("pending credential serializes"),
    );
    (jar.add(cookie), Json(payload)).into_response()
}

/// `POST /api/v1/credentials/enroll/finish`: verify and persist the credential.
///
/// The session must still belong to the user the enrollment began for, so a
/// credential can never be attached to a different account than the one that
/// started (and holds the ceremony cookie).
pub async fn enroll_finish(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    jar: CookieJar,
    Json(body): Json<FinishBody>,
) -> Response {
    let Some(pending) = read_pending(&jar) else {
        return (StatusCode::BAD_REQUEST, "no enrollment in progress").into_response();
    };
    let jar = jar.remove(cookies::clearing(CREDENTIAL_COOKIE));

    // The ceremony must complete on the same account it began on.
    match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(user_id)) if user_id == pending.user_id => {}
        Ok(_) => return StatusCode::FORBIDDEN.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    let credential = match enrollment::verify(&state, &pending.credential, &body) {
        Ok(credential) => credential,
        Err(response) => return response,
    };

    let mut tx = match state.pool.begin().await {
        Ok(tx) => tx,
        Err(error) => {
            tracing::error!(error = %error, "failed to open transaction");
            return (jar, StatusCode::INTERNAL_SERVER_ERROR).into_response();
        }
    };
    if let Err(error) = enrollment::insert(&mut tx, pending.user_id, &credential).await {
        tracing::error!(error = %error, "failed to insert credential");
        return (jar, StatusCode::INTERNAL_SERVER_ERROR).into_response();
    }
    if let Err(error) = tx.commit().await {
        tracing::error!(error = %error, "failed to commit credential");
        return (jar, StatusCode::INTERNAL_SERVER_ERROR).into_response();
    }

    (jar, StatusCode::NO_CONTENT).into_response()
}

/// Reads the in-flight add-credential state from the cookie, if present and valid.
fn read_pending(jar: &CookieJar) -> Option<Pending> {
    jar.get(CREDENTIAL_COOKIE)
        .and_then(|entry| serde_json::from_str(entry.value()).ok())
}
