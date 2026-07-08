//! Server-side sessions.
//!
//! The browser holds only an opaque, HttpOnly cookie carrying a session id; the
//! session references the authenticated user (`migrations/0002_sessions.sql`).
//! [`SessionUser`] is the Axum extractor that turns the cookie into an
//! authenticated [`Principal`]; any handler that takes it is protected and rejects
//! an unauthenticated (or expired) request with `401`.

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

/// The authenticated principal, projected from the user the session references.
#[derive(Debug, Clone)]
pub struct Principal {
    pub sub: String,
    pub email: String,
    pub username: String,
}

/// Creates a session for a user and returns its id (the cookie value).
///
/// # Errors
/// Returns an error if the insert fails.
pub async fn create(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Uuid> {
    sqlx::query_scalar::<_, Uuid>("INSERT INTO sessions (user_id) VALUES ($1) RETURNING id")
        .bind(user_id)
        .fetch_one(pool)
        .await
}

/// Returns the principal for a live (unexpired) session, if one exists.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn principal(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<Principal>> {
    let row = sqlx::query_as::<_, (String, String, String)>(
        "SELECT u.subject, u.email, u.display_name
           FROM sessions s
           JOIN users u ON u.id = s.user_id
          WHERE s.id = $1 AND s.expires_at > now()",
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

/// Deletes a session (logout).
///
/// # Errors
/// Returns an error if the delete fails.
pub async fn delete(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
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
