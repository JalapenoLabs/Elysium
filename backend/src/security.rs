//! Security Center: the account owner's read-only view of their own security state.
//!
//! Backs the settings security surface. For now it exposes the caller's own
//! sign-in history from the login-event audit trail (`login_events`); listing and
//! removing credentials and revoking active sessions are later additions on the
//! same module. Everything here is scoped to the authenticated caller, never
//! another account.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{DateTime, Utc};
use ipnetwork::IpNetwork;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::session::{self, SessionUser, SESSION_COOKIE};
use crate::state::AppState;
use crate::{cookies, recovery, totp, users};

/// Short-lived cookie carrying the unconfirmed TOTP secret between begin and confirm.
const TOTP_ENROLL_COOKIE: &str = "elysium_totp_enroll";

/// How many recent sign-in events the history returns.
const HISTORY_LIMIT: i64 = 50;

/// `GET /api/v1/me/credentials`: the caller's enrolled credentials.
///
/// Lists the account's passkeys (with when each was added and last used) and
/// whether a password and an authenticator (TOTP) are set, so the Security Center
/// can show what the account can sign in with. Never returns secret material.
pub async fn credentials(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
) -> Response {
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "credentials user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let passkeys = sqlx::query_as::<_, (uuid::Uuid, DateTime<Utc>, Option<DateTime<Utc>>)>(
        "SELECT id, created_at, last_used_at FROM webauthn_credentials
          WHERE user_id = $1 ORDER BY created_at ASC",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await;
    let has_password = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM user_password_credentials WHERE user_id = $1)",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await;
    let has_totp = users::has_totp(&state.pool, user_id).await;

    match (passkeys, has_password, has_totp) {
        (Ok(passkeys), Ok(has_password), Ok(has_totp)) => Json(json!({
            "passkeys": passkeys
                .iter()
                .map(|(id, created_at, last_used_at)| json!({
                    "id": id,
                    "created_at": created_at.to_rfc3339(),
                    "last_used_at": last_used_at.map(|at| at.to_rfc3339()),
                }))
                .collect::<Vec<_>>(),
            "password": { "enrolled": has_password },
            "totp": { "enrolled": has_totp },
        }))
        .into_response(),
        _ => {
            tracing::error!("credentials read failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// `GET /api/v1/me/login-history`: the caller's recent sign-in attempts.
///
/// Returns only the authenticated user's own events, most recent first.
pub async fn login_history(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
) -> Response {
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "login-history user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let rows = sqlx::query_as::<_, (String, String, Option<IpNetwork>, String, DateTime<Utc>)>(
        "SELECT method, outcome, ip_address, user_agent, created_at
           FROM login_events
          WHERE user_id = $1
          ORDER BY created_at DESC
          LIMIT $2",
    )
    .bind(user_id)
    .bind(HISTORY_LIMIT)
    .fetch_all(&state.pool)
    .await;

    match rows {
        Ok(rows) => Json(json!({
            "events": rows
                .iter()
                .map(|(method, outcome, ip, user_agent, created_at)| json!({
                    "method": method,
                    "outcome": outcome,
                    "ip": ip.map(|net| net.ip().to_string()),
                    "user_agent": user_agent,
                    "created_at": created_at.to_rfc3339(),
                }))
                .collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "login-history query failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// `POST /api/v1/me/recovery-codes`: regenerate the caller's recovery codes.
///
/// Replaces every existing code with a fresh single-use set in one transaction,
/// so old codes stop working the moment new ones are issued. The plaintext codes
/// are returned exactly once (only their hashes are stored); the client must show
/// them to the user immediately.
pub async fn regenerate_recovery_codes(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
) -> Response {
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "recovery regenerate user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let codes = recovery::generate();
    let outcome = async {
        let mut tx = state.pool.begin().await?;
        sqlx::query("DELETE FROM recovery_codes WHERE user_id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        recovery::store(&mut tx, user_id, &codes).await?;
        tx.commit().await
    }
    .await;

    match outcome {
        Ok(()) => Json(json!({ "recovery_codes": codes })).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "recovery regenerate failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /api/v1/me/totp/begin`: start adding a TOTP second factor.
///
/// Mints a fresh secret, returns its `otpauth://` provisioning URI (render it as a
/// QR code), and carries the unconfirmed secret in a short-lived cookie until
/// [`totp_confirm`] verifies a code. Nothing is stored until confirmation.
pub async fn totp_begin(
    State(_state): State<AppState>,
    SessionUser(principal): SessionUser,
    jar: CookieJar,
) -> Response {
    let secret = totp::generate_secret();
    let uri = match totp::provisioning_uri(&secret, &principal.email) {
        Ok(uri) => uri,
        Err(error) => {
            tracing::error!(error = %error, "totp provisioning failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let cookie = cookies::build(TOTP_ENROLL_COOKIE, STANDARD.encode(&secret));
    (jar.add(cookie), Json(json!({ "provisioning_uri": uri }))).into_response()
}

/// Body of the TOTP confirm request.
#[derive(Deserialize)]
pub struct TotpConfirm {
    code: String,
}

/// `POST /api/v1/me/totp/confirm`: verify a code and store the TOTP factor.
///
/// Confirms the authenticator holds the secret from [`totp_begin`], then seals and
/// stores it (upserting, so this also re-keys an existing authenticator). Sealed at
/// rest under the account's id, like every TOTP secret (see [`crate::crypto`]).
pub async fn totp_confirm(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    jar: CookieJar,
    Json(body): Json<TotpConfirm>,
) -> Response {
    let Some(secret) = jar
        .get(TOTP_ENROLL_COOKIE)
        .and_then(|entry| STANDARD.decode(entry.value()).ok())
    else {
        return (StatusCode::BAD_REQUEST, "no enrollment in progress").into_response();
    };
    if !totp::verify(&secret, &body.code).unwrap_or(false) {
        return (StatusCode::BAD_REQUEST, "invalid code").into_response();
    }

    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "totp confirm user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let sealed = state.crypto.seal(&secret, user_id.as_bytes());
    let result = sqlx::query(
        "INSERT INTO user_totp_secrets (user_id, secret, confirmed_at)
         VALUES ($1, $2, now())
         ON CONFLICT (user_id) DO UPDATE SET secret = EXCLUDED.secret, confirmed_at = now()",
    )
    .bind(user_id)
    .bind(sealed)
    .execute(&state.pool)
    .await;

    match result {
        Ok(_) => {
            let jar = jar.remove(cookies::clearing(TOTP_ENROLL_COOKIE));
            (jar, StatusCode::NO_CONTENT).into_response()
        }
        Err(error) => {
            tracing::error!(error = %error, "totp confirm store failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `GET /api/v1/me/sessions`: the caller's active sessions across devices.
///
/// Each entry carries the device (user agent), origin IP, when it started and was
/// last active, when it expires, and whether it is the session making this request.
pub async fn sessions(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    jar: CookieJar,
) -> Response {
    let current = session::session_id(&jar);
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "sessions user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let rows = sqlx::query_as::<
        _,
        (
            Uuid,
            String,
            Option<IpNetwork>,
            DateTime<Utc>,
            DateTime<Utc>,
            DateTime<Utc>,
        ),
    >(
        "SELECT s.id, s.user_agent, s.ip_address, s.created_at, s.last_seen_at, s.expires_at
           FROM sessions s
           JOIN session_accounts sa ON sa.session_id = s.id
          WHERE sa.user_id = $1 AND s.expires_at > now()
          ORDER BY s.last_seen_at DESC",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await;

    match rows {
        Ok(rows) => Json(json!({
            "sessions": rows
                .iter()
                .map(|(id, user_agent, ip, created_at, last_seen_at, expires_at)| json!({
                    "id": id,
                    "user_agent": user_agent,
                    "ip": ip.map(|net| net.ip().to_string()),
                    "created_at": created_at.to_rfc3339(),
                    "last_seen_at": last_seen_at.to_rfc3339(),
                    "expires_at": expires_at.to_rfc3339(),
                    "current": Some(*id) == current,
                }))
                .collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "sessions read failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// `DELETE /api/v1/me/sessions/:session_id`: sign one of the caller's sessions out.
///
/// Only a session the caller is part of can be revoked; revoking the current one
/// signs this client out and clears its cookie.
pub async fn revoke_session(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    jar: CookieJar,
    Path(session_id): Path<Uuid>,
) -> Response {
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "revoke-session user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let affected = sqlx::query(
        "DELETE FROM sessions
          WHERE id = $1 AND id IN (SELECT session_id FROM session_accounts WHERE user_id = $2)",
    )
    .bind(session_id)
    .bind(user_id)
    .execute(&state.pool)
    .await;
    match affected {
        Ok(result) if result.rows_affected() == 0 => StatusCode::NOT_FOUND.into_response(),
        Ok(_) => {
            if session::session_id(&jar) == Some(session_id) {
                return (
                    jar.remove(cookies::clearing(SESSION_COOKIE)),
                    StatusCode::NO_CONTENT,
                )
                    .into_response();
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => {
            tracing::error!(error = %error, "session revoke failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /api/v1/me/sessions/revoke-others`: sign out every session but this one.
pub async fn revoke_other_sessions(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    jar: CookieJar,
) -> Response {
    let Some(current) = session::session_id(&jar) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "revoke-others user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let result = sqlx::query(
        "DELETE FROM sessions
          WHERE id <> $1 AND id IN (SELECT session_id FROM session_accounts WHERE user_id = $2)",
    )
    .bind(current)
    .bind(user_id)
    .execute(&state.pool)
    .await;
    match result {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "revoke-others failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
