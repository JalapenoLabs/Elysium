//! Recovery codes: one-time secrets for regaining access after a lost device.
//!
//! A set is issued at every credential enrollment (claim and invite acceptance)
//! and shown to the user exactly once; only SHA-256 hashes are stored. A code is
//! consumed on a recovery sign-in, which re-establishes a session so the user can
//! enroll a fresh credential (see `crate::credentials`). Codes are high-entropy
//! and single-use, so a fast hash is sufficient; the security is in the entropy.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::login_events::{self, ClientInfo};
use crate::rate_limit::{self, Scope};
use crate::session;
use crate::state::AppState;
use crate::users;

/// Number of recovery codes issued per enrollment.
const CODE_COUNT: usize = 10;

/// Random bytes per code (64 bits: ample for a single-use, rate-limited secret).
const CODE_BYTES: usize = 8;

/// Generates a fresh set of recovery codes as human-typable strings.
///
/// Codes are uppercase hex grouped in fours (e.g. `A1B2-C3D4-E5F6-7890`), which
/// avoids ambiguous characters and is easy to read back.
pub fn generate() -> Vec<String> {
    (0..CODE_COUNT)
        .map(|_| {
            let mut bytes = [0u8; CODE_BYTES];
            OsRng.fill_bytes(&mut bytes);
            let hex: String = bytes.iter().map(|byte| format!("{byte:02X}")).collect();
            hex.as_bytes()
                .chunks(4)
                .map(|chunk| std::str::from_utf8(chunk).expect("hex is ascii"))
                .collect::<Vec<_>>()
                .join("-")
        })
        .collect()
}

/// Stores the hashes of a code set for a user, on the caller's connection.
///
/// # Errors
/// Returns an error if the insert fails.
pub async fn store(conn: &mut PgConnection, user_id: Uuid, codes: &[String]) -> sqlx::Result<()> {
    for code in codes {
        sqlx::query("INSERT INTO recovery_codes (user_id, code_hash) VALUES ($1, $2)")
            .bind(user_id)
            .bind(hash(code))
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// Consumes one unused recovery code for a user, returning whether it matched.
///
/// The match-and-mark is a single atomic `UPDATE`, so a code can be redeemed at
/// most once even under concurrent attempts.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn consume(pool: &PgPool, user_id: Uuid, code: &str) -> sqlx::Result<bool> {
    let affected = sqlx::query(
        "UPDATE recovery_codes SET used_at = now()
          WHERE user_id = $1 AND code_hash = $2 AND used_at IS NULL",
    )
    .bind(user_id)
    .bind(hash(code))
    .execute(pool)
    .await?
    .rows_affected();
    Ok(affected == 1)
}

/// Body of the recovery sign-in request.
#[derive(Deserialize)]
pub struct RecoveryRequest {
    email: String,
    code: String,
}

/// `POST /api/v1/auth/recovery/authenticate`: sign in with a recovery code.
///
/// Consumes one unused code and establishes a session. An unknown email and a
/// wrong code are the same generic `401`, so the endpoint never reveals which
/// accounts exist. The recovered session lets the user enroll a fresh credential
/// (see `crate::credentials`), which is the intended follow-up after a lost device.
pub async fn authenticate(
    State(state): State<AppState>,
    jar: CookieJar,
    client: ClientInfo,
    Json(body): Json<RecoveryRequest>,
) -> Response {
    // High-entropy codes resist guessing, but throttle anyway so the endpoint is
    // not a free oracle for hammering; scope by the presented email.
    match rate_limit::is_throttled(
        &state.pool,
        Scope::Email(body.email.trim()),
        login_events::Method::Recovery,
    )
    .await
    {
        Ok(true) => return StatusCode::TOO_MANY_REQUESTS.into_response(),
        Ok(false) => {}
        Err(error) => {
            tracing::error!(error = %error, "recovery rate-limit check failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    let user = match users::find_by_email(&state.pool, body.email.trim()).await {
        Ok(user) => user,
        Err(error) => {
            tracing::error!(error = %error, "recovery user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let redeemed = match &user {
        Some(user) => consume(&state.pool, user.id, &body.code)
            .await
            .unwrap_or(false),
        None => false,
    };

    if !redeemed {
        login_events::record(
            &state.pool,
            user.as_ref().map(|user| user.id),
            body.email.trim(),
            login_events::Method::Recovery,
            login_events::Outcome::Failure,
            &client,
        )
        .await;
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let user_id = user.expect("redeemed implies a known user").id;
    login_events::record(
        &state.pool,
        Some(user_id),
        body.email.trim(),
        login_events::Method::Recovery,
        login_events::Outcome::Success,
        &client,
    )
    .await;
    session::establish(&state.pool, jar, user_id, &client).await
}

/// Normalizes then SHA-256-hashes a code to its stored hex form.
///
/// Normalization strips the display grouping (dashes, spaces) and upper-cases, so
/// a user can type the code with or without its separators.
fn hash(code: &str) -> String {
    let normalized: String = code
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_uppercase();
    let digest = Sha256::digest(normalized.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
