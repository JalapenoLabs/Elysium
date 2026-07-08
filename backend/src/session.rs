//! Server-side sessions for the Backend-for-Frontend auth model.
//!
//! The browser holds only an opaque, HttpOnly cookie carrying a session id. The
//! real Keycloak tokens live in Postgres (see `migrations/0002_sessions.sql`) and
//! are never exposed to JavaScript. [`SessionUser`] is the Axum extractor that
//! turns the cookie into an authenticated [`Principal`]; any handler that takes it
//! is protected and rejects an unauthenticated request with `401`.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::CookieJar;
use sqlx::PgPool;
use uuid::Uuid;

use crate::state::AppState;

/// Name of the HttpOnly cookie carrying the opaque session id.
pub const SESSION_COOKIE: &str = "elysium_session";

/// The authenticated principal projected from a session at login time.
#[derive(Debug, Clone)]
pub struct Principal {
    pub sub: String,
    pub email: String,
    pub username: String,
}

/// Identity and tokens captured at the OIDC callback, persisted as a session.
#[derive(Debug)]
pub struct NewSession {
    pub sub: String,
    pub email: String,
    pub username: String,
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    /// Seconds until the access token expires, straight from the token response.
    pub expires_in: i64,
}

/// Creates a session row and returns its id, which becomes the cookie value.
///
/// # Errors
/// Returns an error if the insert fails.
pub async fn create(pool: &PgPool, session: NewSession) -> sqlx::Result<Uuid> {
    sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO sessions
           (keycloak_sub, email, username, access_token, refresh_token, id_token, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, now() + make_interval(secs => $7))
         RETURNING id",
    )
    .bind(&session.sub)
    .bind(&session.email)
    .bind(&session.username)
    .bind(&session.access_token)
    .bind(&session.refresh_token)
    .bind(&session.id_token)
    .bind(session.expires_in as f64)
    .fetch_one(pool)
    .await
}

/// Returns the principal for a session id, if the session exists.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn principal(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<Principal>> {
    let row = sqlx::query_as::<_, (String, String, String)>(
        "SELECT keycloak_sub, email, username FROM sessions WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(sub, email, username)| Principal {
        sub,
        email,
        username,
    }))
}

/// Deletes a session (logout), returning its `(id_token, refresh_token)` so the
/// caller can end the Keycloak session too.
///
/// # Errors
/// Returns an error if the delete fails.
pub async fn delete(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<(String, String)>> {
    sqlx::query_as::<_, (String, String)>(
        "DELETE FROM sessions WHERE id = $1 RETURNING id_token, refresh_token",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// Axum extractor for a request authenticated by a valid session cookie.
#[derive(Debug, Clone)]
pub struct SessionUser(pub Principal);

impl FromRequestParts<AppState> for SessionUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);

        let Some(raw) = jar
            .get(SESSION_COOKIE)
            .map(|entry| entry.value().to_owned())
        else {
            return Err(StatusCode::UNAUTHORIZED.into_response());
        };
        let Ok(id) = Uuid::parse_str(&raw) else {
            return Err(StatusCode::UNAUTHORIZED.into_response());
        };

        match principal(&state.pool, id).await {
            Ok(Some(principal)) => Ok(SessionUser(principal)),
            Ok(None) => Err(StatusCode::UNAUTHORIZED.into_response()),
            Err(error) => {
                tracing::error!(error = %error, "session lookup failed");
                Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
            }
        }
    }
}
