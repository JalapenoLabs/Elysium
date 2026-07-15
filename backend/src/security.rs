//! Security Center: the account owner's view of their own security state.
//!
//! Backs the settings security surface, every endpoint scoped to the authenticated
//! caller and never another account: their credentials, one combined read of active
//! sessions and recent sign-ins (`security_activity`, protobuf), recovery-code
//! regeneration, self-service TOTP enrollment, and session revocation.

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

use crate::proto::common::v1::Timestamp;
use crate::proto::security::v1 as pb;
use crate::protobuf::Protobuf;
use crate::session::{self, SessionUser, SESSION_COOKIE};
use crate::state::AppState;
use crate::{cookies, geoip, recovery, totp, users};

/// Short-lived cookie carrying the unconfirmed TOTP secret between begin and confirm.
const TOTP_ENROLL_COOKIE: &str = "elysium_totp_enroll";

/// How many recent sign-in events the history returns.
const HISTORY_LIMIT: i64 = 50;

/// `GET /api/v1/me/credentials`: the caller's enrolled credentials.
///
/// Lists the account's named passkeys and TOTP authenticators (each with when it was
/// added and last used, and which authenticator is the default) plus whether a
/// password is set, so the Security Center can show and manage what the account signs
/// in with. Never returns secret material.
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

    let passkeys = sqlx::query_as::<_, (uuid::Uuid, String, DateTime<Utc>, Option<DateTime<Utc>>)>(
        "SELECT id, name, created_at, last_used_at FROM webauthn_credentials
              WHERE user_id = $1 ORDER BY created_at ASC",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await;
    let totp_methods = sqlx::query_as::<
        _,
        (
            uuid::Uuid,
            String,
            bool,
            DateTime<Utc>,
            Option<DateTime<Utc>>,
        ),
    >(
        "SELECT id, name, is_default, created_at, last_used_at FROM user_totp_secrets
              WHERE user_id = $1 AND confirmed_at IS NOT NULL ORDER BY created_at ASC",
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

    match (passkeys, totp_methods, has_password) {
        (Ok(passkeys), Ok(totp_methods), Ok(has_password)) => Json(json!({
            "passkeys": passkeys
                .iter()
                .map(|(id, name, created_at, last_used_at)| json!({
                    "id": id,
                    "name": name,
                    "created_at": created_at.to_rfc3339(),
                    "last_used_at": last_used_at.map(|at| at.to_rfc3339()),
                }))
                .collect::<Vec<_>>(),
            "totp_methods": totp_methods
                .iter()
                .map(|(id, name, is_default, created_at, last_used_at)| json!({
                    "id": id,
                    "name": name,
                    "is_default": is_default,
                    "created_at": created_at.to_rfc3339(),
                    "last_used_at": last_used_at.map(|at| at.to_rfc3339()),
                }))
                .collect::<Vec<_>>(),
            "password": { "enrolled": has_password },
        }))
        .into_response(),
        _ => {
            tracing::error!("credentials read failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// `GET /api/v1/me/security-activity`: the caller's sessions and recent sign-ins.
///
/// One protobuf response for the one settings page that shows both: every unexpired
/// session across the account's devices (newest activity first) and the recent
/// sign-in attempts from the audit trail (newest first). Each entry carries the
/// coarse geolocation of its IP when resolved (see `crate::geoip`). Scoped to the
/// authenticated caller, never another account.
pub async fn security_activity(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    jar: CookieJar,
) -> Response {
    let current = session::session_id(&jar);
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "security-activity user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let session_rows = sqlx::query_as::<
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

    let event_rows =
        sqlx::query_as::<_, (String, String, Option<IpNetwork>, String, DateTime<Utc>)>(
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

    let (session_rows, event_rows) = match (session_rows, event_rows) {
        (Ok(sessions), Ok(events)) => (sessions, events),
        (Err(error), _) | (_, Err(error)) => {
            tracing::error!(error = %error, "security-activity query failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let mut sessions = Vec::with_capacity(session_rows.len());
    for (id, user_agent, ip, created_at, last_seen_at, expires_at) in &session_rows {
        let addr = ip.map(|net| net.ip());
        sessions.push(pb::Session {
            id: id.to_string(),
            user_agent: user_agent.clone(),
            ip: addr.map(|addr| addr.to_string()),
            location: geoip::locate(&state, addr).await.map(into_location),
            created_at: Some(to_timestamp(*created_at)),
            last_seen_at: Some(to_timestamp(*last_seen_at)),
            expires_at: Some(to_timestamp(*expires_at)),
            current: Some(*id) == current,
        });
    }

    let mut recent_sign_ins = Vec::with_capacity(event_rows.len());
    for (method, outcome, ip, user_agent, created_at) in &event_rows {
        let addr = ip.map(|net| net.ip());
        recent_sign_ins.push(pb::SignInEvent {
            method: method.clone(),
            outcome: outcome.clone(),
            ip: addr.map(|addr| addr.to_string()),
            location: geoip::locate(&state, addr).await.map(into_location),
            created_at: Some(to_timestamp(*created_at)),
            user_agent: user_agent.clone(),
        });
    }

    Protobuf(pb::SecurityActivity {
        sessions,
        recent_sign_ins,
    })
    .into_response()
}

/// Converts a resolved geolocation into its protobuf form.
fn into_location(location: geoip::Location) -> pb::Location {
    pb::Location {
        city: location.city,
        country: location.country,
        country_code: location.country_code,
    }
}

/// Builds a UTC protobuf `Timestamp` from a `chrono` instant; the zone is display-only.
fn to_timestamp(instant: DateTime<Utc>) -> Timestamp {
    Timestamp {
        epoch_seconds: instant.timestamp(),
        nanos: instant.timestamp_subsec_nanos(),
        timezone: "Etc/UTC".to_string(),
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
    /// User label for the authenticator; defaults to "Authenticator" when blank.
    #[serde(default)]
    name: String,
}

/// `POST /api/v1/me/totp/confirm`: verify a code and add a named TOTP authenticator.
///
/// Confirms the authenticator holds the secret from [`totp_begin`], then seals and
/// stores it as a new named authenticator (the user may hold several). It becomes the
/// default only when the user has none yet. Sealed at rest under the account's id,
/// like every TOTP secret (see [`crate::crypto`]).
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

    let name = match body.name.trim() {
        "" => "Authenticator",
        chosen => chosen,
    };
    let sealed = state.crypto.seal(&secret, user_id.as_bytes());
    let result = sqlx::query(
        "INSERT INTO user_totp_secrets (user_id, name, secret, is_default, confirmed_at)
         VALUES ($1, $2, $3,
                 NOT EXISTS (
                     SELECT 1 FROM user_totp_secrets WHERE user_id = $1 AND is_default
                 ),
                 now())",
    )
    .bind(user_id)
    .bind(name)
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

/// A new label for a credential (a TOTP authenticator or a passkey).
#[derive(Deserialize)]
pub struct RenameCredential {
    name: String,
}

/// Resolves the caller's user id, or returns a ready error response.
async fn resolve_user(state: &AppState, subject: &str) -> Result<Uuid, Response> {
    match users::id_for_subject(&state.pool, subject).await {
        Ok(Some(id)) => Ok(id),
        Ok(None) => Err(StatusCode::UNAUTHORIZED.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "credential-management user lookup failed");
            Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
    }
}

/// Maps a scoped mutation to 204 (done), 404 (nothing matched), or 503 (error).
fn respond_affected(
    result: sqlx::Result<sqlx::postgres::PgQueryResult>,
    operation: &str,
) -> Response {
    match result {
        Ok(done) if done.rows_affected() > 0 => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(error = %error, operation, "credential mutation failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// `PATCH /api/v1/me/totp/{id}`: rename one of the caller's TOTP authenticators.
pub async fn rename_totp(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(id): Path<Uuid>,
    Json(body): Json<RenameCredential>,
) -> Response {
    let name = body.name.trim();
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, "name is required").into_response();
    }
    let user_id = match resolve_user(&state, &principal.sub).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    let result =
        sqlx::query("UPDATE user_totp_secrets SET name = $1 WHERE id = $2 AND user_id = $3")
            .bind(name)
            .bind(id)
            .bind(user_id)
            .execute(&state.pool)
            .await;
    respond_affected(result, "totp.rename")
}

/// `POST /api/v1/me/totp/{id}/default`: make a TOTP authenticator the default.
///
/// Clears the current default and sets the new one in a transaction, so the
/// one-default-per-user invariant is never transiently violated.
pub async fn set_default_totp(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(id): Path<Uuid>,
) -> Response {
    let user_id = match resolve_user(&state, &principal.sub).await {
        Ok(id) => id,
        Err(response) => return response,
    };

    let outcome = async {
        let mut tx = state.pool.begin().await?;
        let owned = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                 SELECT 1 FROM user_totp_secrets
                  WHERE id = $1 AND user_id = $2 AND confirmed_at IS NOT NULL
             )",
        )
        .bind(id)
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
        if !owned {
            return Ok::<bool, sqlx::Error>(false);
        }
        sqlx::query(
            "UPDATE user_totp_secrets SET is_default = FALSE WHERE user_id = $1 AND is_default",
        )
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE user_totp_secrets SET is_default = TRUE WHERE id = $1 AND user_id = $2",
        )
        .bind(id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }
    .await;

    match outcome {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "set default totp failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// `DELETE /api/v1/me/totp/{id}`: remove one of the caller's TOTP authenticators.
///
/// If the removed one was the default and others remain, the oldest is promoted so
/// the account keeps a default. Removing the last leaves the account with no TOTP; a
/// `require_totp` org re-prompts enrollment on the next password sign-in (see
/// `crate::context`).
pub async fn remove_totp(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(id): Path<Uuid>,
) -> Response {
    let user_id = match resolve_user(&state, &principal.sub).await {
        Ok(id) => id,
        Err(response) => return response,
    };

    let outcome = async {
        let mut tx = state.pool.begin().await?;
        let removed = sqlx::query_scalar::<_, bool>(
            "DELETE FROM user_totp_secrets WHERE id = $1 AND user_id = $2 RETURNING is_default",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(was_default) = removed else {
            return Ok::<bool, sqlx::Error>(false);
        };
        if was_default {
            sqlx::query(
                "UPDATE user_totp_secrets SET is_default = TRUE
                  WHERE id = (
                      SELECT id FROM user_totp_secrets
                       WHERE user_id = $1 AND confirmed_at IS NOT NULL
                       ORDER BY created_at ASC
                       LIMIT 1
                  )",
            )
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(true)
    }
    .await;

    match outcome {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "remove totp failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// `PATCH /api/v1/me/passkeys/{id}`: rename one of the caller's passkeys.
pub async fn rename_passkey(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(id): Path<Uuid>,
    Json(body): Json<RenameCredential>,
) -> Response {
    let user_id = match resolve_user(&state, &principal.sub).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    let result =
        sqlx::query("UPDATE webauthn_credentials SET name = $1 WHERE id = $2 AND user_id = $3")
            .bind(body.name.trim())
            .bind(id)
            .bind(user_id)
            .execute(&state.pool)
            .await;
    respond_affected(result, "passkey.rename")
}

/// The result of attempting to remove a passkey.
enum PasskeyRemoval {
    Done,
    NotFound,
    /// Would leave the account with no way to sign in.
    LastCredential,
}

/// `DELETE /api/v1/me/passkeys/{id}`: remove one of the caller's passkeys.
///
/// Refuses to remove the account's last sign-in credential: a passkey cannot be
/// deleted when it is the user's only passkey and they have no password, which would
/// otherwise lock them out.
pub async fn remove_passkey(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(id): Path<Uuid>,
) -> Response {
    let user_id = match resolve_user(&state, &principal.sub).await {
        Ok(id) => id,
        Err(response) => return response,
    };

    let outcome = async {
        let mut tx = state.pool.begin().await?;
        let owned = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM webauthn_credentials WHERE id = $1 AND user_id = $2)",
        )
        .bind(id)
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
        if !owned {
            return Ok::<PasskeyRemoval, sqlx::Error>(PasskeyRemoval::NotFound);
        }
        let passkeys = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM webauthn_credentials WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
        let has_password = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM user_password_credentials WHERE user_id = $1)",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
        if passkeys <= 1 && !has_password {
            return Ok(PasskeyRemoval::LastCredential);
        }
        sqlx::query("DELETE FROM webauthn_credentials WHERE id = $1 AND user_id = $2")
            .bind(id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(PasskeyRemoval::Done)
    }
    .await;

    match outcome {
        Ok(PasskeyRemoval::Done) => StatusCode::NO_CONTENT.into_response(),
        Ok(PasskeyRemoval::NotFound) => StatusCode::NOT_FOUND.into_response(),
        Ok(PasskeyRemoval::LastCredential) => (
            StatusCode::CONFLICT,
            "cannot remove your only sign-in credential",
        )
            .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "remove passkey failed");
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
