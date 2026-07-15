//! Multi-account server-side sessions.
//!
//! A session is a browser session that can hold several signed-in accounts with
//! one active (Google-style): the opaque HttpOnly `elysium_session` cookie carries
//! the session id, `session_accounts` lists the profiles signed into it, and
//! `sessions.active_user_id` / `active_org_id` say which account (and which of its
//! orgs) is currently in use. [`SessionUser`] resolves the active account into a
//! [`Principal`]; any handler that takes it is protected. See `docs/identity.md`.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::CookieJar;
use ipnetwork::IpNetwork;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::cookies;
use crate::login_events::ClientInfo;
use crate::state::AppState;
use crate::users;

/// Name of the HttpOnly cookie carrying the opaque session id.
pub const SESSION_COOKIE: &str = "elysium_session";

/// The authenticated principal: the session's active account, with its active org.
#[derive(Debug, Clone)]
pub struct Principal {
    pub sub: String,
    pub email: String,
    pub username: String,
    /// The org this account is currently acting in, if any (the org switcher).
    pub active_org_id: Option<Uuid>,
}

/// Reads and parses the session id from the cookie jar, if present and well-formed.
pub fn session_id(jar: &CookieJar) -> Option<Uuid> {
    jar.get(SESSION_COOKIE)
        .and_then(|entry| Uuid::parse_str(entry.value()).ok())
}

/// Whether `session_id` is a live (unexpired) session that includes `user_id`.
async fn holds_account(pool: &PgPool, session_id: Uuid, user_id: Uuid) -> sqlx::Result<bool> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1 FROM session_accounts sa
               JOIN sessions s ON s.id = sa.session_id
              WHERE sa.session_id = $1 AND sa.user_id = $2 AND s.expires_at > now()
         )",
    )
    .bind(session_id)
    .bind(user_id)
    .fetch_one(pool)
    .await
}

/// Signs `user_id` into the request's session and makes it active.
///
/// If the request already carries a live session cookie, the account is appended
/// to it (multiple simultaneous accounts); otherwise a fresh session is created.
/// Either way the account becomes active and the response carries the cookie.
pub async fn establish(
    pool: &PgPool,
    jar: CookieJar,
    user_id: Uuid,
    client: &ClientInfo,
) -> Response {
    let existing = match session_id(&jar) {
        Some(id) => match live(pool, id).await {
            Ok(alive) => alive.then_some(id),
            Err(error) => {
                tracing::error!(error = %error, "session liveness check failed");
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
        },
        None => None,
    };

    let result = match existing {
        Some(id) => add_account(pool, id, user_id).await.map(|()| id),
        None => create(pool, user_id, client).await,
    };

    match result {
        Ok(session_id) => {
            let cookie = cookies::build(SESSION_COOKIE, session_id.to_string());
            (jar.add(cookie), StatusCode::NO_CONTENT).into_response()
        }
        Err(error) => {
            tracing::error!(error = %error, "failed to establish session");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Whether a session exists and has not expired.
async fn live(pool: &PgPool, id: Uuid) -> sqlx::Result<bool> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM sessions WHERE id = $1 AND expires_at > now())",
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

/// Creates a new session with `user_id` as its sole, active account.
///
/// # Errors
/// Returns an error if the inserts fail.
async fn create(pool: &PgPool, user_id: Uuid, client: &ClientInfo) -> sqlx::Result<Uuid> {
    let org_id = users::default_org(pool, user_id).await?;
    let mut tx = pool.begin().await?;
    let session_id = create_on(&mut tx, user_id, org_id, client).await?;
    tx.commit().await?;
    Ok(session_id)
}

/// Creates a new one-account session on the caller's connection.
///
/// Lets provisioning flows (first-run claim, invite acceptance) mint the session
/// inside the same transaction that creates the profile, with `active_org_id` set
/// to the org just joined.
///
/// # Errors
/// Returns an error if the inserts fail.
pub async fn create_on(
    conn: &mut PgConnection,
    user_id: Uuid,
    org_id: Option<Uuid>,
    client: &ClientInfo,
) -> sqlx::Result<Uuid> {
    // Honor the strictest session lifetime across the user's orgs' policies.
    let lifetime = lifetime_seconds(&mut *conn, user_id).await?;
    let session_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO sessions (active_user_id, active_org_id, expires_at, user_agent, ip_address)
         VALUES ($1, $2, now() + make_interval(secs => $3), $4, $5) RETURNING id",
    )
    .bind(user_id)
    .bind(org_id)
    .bind(lifetime as f64)
    .bind(&client.user_agent)
    .bind(client.ip.map(IpNetwork::from))
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query("INSERT INTO session_accounts (session_id, user_id) VALUES ($1, $2)")
        .bind(session_id)
        .bind(user_id)
        .execute(&mut *conn)
        .await?;
    Ok(session_id)
}

/// Default absolute session lifetime (30 days), matching the `sessions` and
/// `org_auth_policies` table defaults; used when the user has no org policy yet.
const DEFAULT_SESSION_LIFETIME_SECONDS: i64 = 30 * 24 * 60 * 60;

/// The strictest (smallest) session lifetime across the user's orgs' policies.
///
/// A profile in several orgs gets the most restrictive one, so no org's policy can
/// be undercut by membership in a laxer org. Falls back to the default when the
/// user has no org yet.
///
/// # Errors
/// Returns an error if the query fails.
async fn lifetime_seconds(conn: &mut PgConnection, user_id: Uuid) -> sqlx::Result<i64> {
    let seconds = sqlx::query_scalar::<_, Option<i32>>(
        "SELECT min(p.session_lifetime_seconds)
           FROM org_members m
           JOIN org_auth_policies p ON p.org_id = m.org_id
          WHERE m.user_id = $1",
    )
    .bind(user_id)
    .fetch_one(conn)
    .await?;
    Ok(seconds.map_or(DEFAULT_SESSION_LIFETIME_SECONDS, i64::from))
}

/// Adds `user_id` to an existing session and makes it the active account.
///
/// # Errors
/// Returns an error if the writes fail.
pub async fn add_account(pool: &PgPool, session_id: Uuid, user_id: Uuid) -> sqlx::Result<()> {
    let org_id = users::default_org(pool, user_id).await?;
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO session_accounts (session_id, user_id) VALUES ($1, $2)
         ON CONFLICT (session_id, user_id) DO NOTHING",
    )
    .bind(session_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE sessions SET active_user_id = $1, active_org_id = $2 WHERE id = $3")
        .bind(user_id)
        .bind(org_id)
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Switches the active account to `user_id`, which must already be in the session.
///
/// Returns whether the switch happened (false when the account is not in the
/// session, so a caller cannot switch to an account it never authenticated).
///
/// # Errors
/// Returns an error if the query fails.
pub async fn switch_account(pool: &PgPool, session_id: Uuid, user_id: Uuid) -> sqlx::Result<bool> {
    if !holds_account(pool, session_id, user_id).await? {
        return Ok(false);
    }
    let org_id = users::default_org(pool, user_id).await?;
    sqlx::query("UPDATE sessions SET active_user_id = $1, active_org_id = $2 WHERE id = $3")
        .bind(user_id)
        .bind(org_id)
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(true)
}

/// Sets the active org for the session's active account.
///
/// Returns whether it happened (false when the active account is not a member of
/// `org_id`).
///
/// # Errors
/// Returns an error if the query fails.
pub async fn switch_org(pool: &PgPool, session_id: Uuid, org_id: Uuid) -> sqlx::Result<bool> {
    let affected = sqlx::query(
        "UPDATE sessions s SET active_org_id = $1
          WHERE s.id = $2
            AND EXISTS (SELECT 1 FROM org_members m
                         WHERE m.user_id = s.active_user_id AND m.org_id = $1)",
    )
    .bind(org_id)
    .bind(session_id)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(affected == 1)
}

/// Removes one account from the session; if it was active, re-points to another.
///
/// Returns `true` if accounts remain (the session lives on), `false` if the last
/// account was removed (the caller should delete the session and clear the cookie).
///
/// # Errors
/// Returns an error if the queries fail.
pub async fn remove_account(pool: &PgPool, session_id: Uuid, user_id: Uuid) -> sqlx::Result<bool> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM session_accounts WHERE session_id = $1 AND user_id = $2")
        .bind(session_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    // Pick any remaining account to become active (its default org follows).
    let next = sqlx::query_scalar::<_, Uuid>(
        "SELECT user_id FROM session_accounts WHERE session_id = $1 ORDER BY added_at ASC LIMIT 1",
    )
    .bind(session_id)
    .fetch_optional(&mut *tx)
    .await?;

    match next {
        Some(next_user) => {
            let org_id = users::default_org(pool, next_user).await?;
            sqlx::query(
                "UPDATE sessions SET active_user_id = $1, active_org_id = $2 WHERE id = $3",
            )
            .bind(next_user)
            .bind(org_id)
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(true)
        }
        None => {
            sqlx::query("DELETE FROM sessions WHERE id = $1")
                .bind(session_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(false)
        }
    }
}

/// Returns the principal for a live session's active account, if any.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn principal(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<Principal>> {
    let row = sqlx::query_as::<_, (String, String, String, Option<Uuid>)>(
        "SELECT u.subject, u.email, u.display_name, s.active_org_id
           FROM sessions s
           JOIN users u ON u.id = s.active_user_id
          WHERE s.id = $1 AND s.expires_at > now()",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(sub, email, username, active_org_id)| Principal {
        sub,
        email,
        username,
        active_org_id,
    }))
}

/// Deletes a session and every account in it (sign out everywhere on this device).
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

        let Some(id) = session_id(&jar) else {
            return Err(StatusCode::UNAUTHORIZED.into_response());
        };

        match principal(&state.pool, id).await {
            Ok(Some(principal)) => {
                touch_last_seen(&state.pool, id).await;
                Ok(SessionUser(principal))
            }
            Ok(None) => Err(StatusCode::UNAUTHORIZED.into_response()),
            Err(error) => {
                tracing::error!(error = %error, "session lookup failed");
                Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
            }
        }
    }
}

/// Refreshes a session's `last_seen_at`, at most once a minute, for the sessions view.
///
/// Fire-and-forget: a failed or skipped update never affects the request, and the
/// `WHERE` clause throttles it to at most one small write per minute per active
/// session rather than one per request.
async fn touch_last_seen(pool: &PgPool, id: Uuid) {
    let _ = sqlx::query(
        "UPDATE sessions SET last_seen_at = now()
          WHERE id = $1 AND last_seen_at < now() - interval '1 minute'",
    )
    .bind(id)
    .execute(pool)
    .await;
}
