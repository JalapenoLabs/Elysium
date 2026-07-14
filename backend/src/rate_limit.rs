//! Sliding-window rate limiting for the verification endpoints.
//!
//! Online-guessing defense shared by every path that checks a presented secret
//! (password, the TOTP second factor, recovery codes). Rather than a separate
//! counter store, the limiter reads the append-only login history
//! ([`crate::login_events`]): it counts recent *failures* for a subject and
//! method within a fixed window and reports whether the threshold is reached.
//!
//! The window is the throttle *and* the lockout: it is temporary and
//! self-expiring, so a locked-out account recovers on its own without an admin
//! unlock (a permanent lock would be a denial-of-service lever). OPAQUE and
//! high-entropy recovery codes already make offline attack infeasible, so this
//! only has to blunt online abuse.

use sqlx::PgPool;
use uuid::Uuid;

use crate::login_events::Method;

/// Failed attempts within [`WINDOW_MINUTES`] before a subject is throttled.
///
/// Deliberately small: the credentials themselves resist offline attack, so this
/// only has to blunt online guessing. Five tolerates a few honest typos while
/// leaving a 6-digit TOTP space (with one step of skew) effectively unguessable.
/// Shared by every verification path, so tune it here, not per endpoint.
pub const MAX_RECENT_FAILURES: i64 = 5;

/// Window over which [`MAX_RECENT_FAILURES`] is counted, in minutes.
///
/// Long enough to matter against scripted guessing, short enough that a locked
/// account recovers quickly on its own.
pub const WINDOW_MINUTES: i64 = 15;

/// Which identity a failure count is scoped to.
///
/// Pre-resolution paths (password, recovery) only know the presented email;
/// the TOTP second factor has already resolved the user, so it scopes by id and
/// counts across every client rather than by a spoofable identifier.
pub enum Scope<'a> {
    /// By presented email, matched case-insensitively.
    Email(&'a str),
    /// By resolved user id.
    User(Uuid),
}

/// Whether `scope` has reached the failure threshold for `method` in the window.
///
/// # Errors
/// Returns an error if the count query fails; callers should treat that as a
/// transient failure (a `503`) rather than letting the attempt through.
pub async fn is_throttled(pool: &PgPool, scope: Scope<'_>, method: Method) -> sqlx::Result<bool> {
    let count = match scope {
        Scope::Email(email) => {
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM login_events
                  WHERE lower(email) = lower($1) AND method = $2 AND outcome = 'failure'
                    AND created_at > now() - make_interval(mins => $3)",
            )
            .bind(email)
            .bind(method.as_str())
            .bind(WINDOW_MINUTES as i32)
            .fetch_one(pool)
            .await?
        }
        Scope::User(user_id) => {
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM login_events
                  WHERE user_id = $1 AND method = $2 AND outcome = 'failure'
                    AND created_at > now() - make_interval(mins => $3)",
            )
            .bind(user_id)
            .bind(method.as_str())
            .bind(WINDOW_MINUTES as i32)
            .fetch_one(pool)
            .await?
        }
    };
    Ok(count >= MAX_RECENT_FAILURES)
}
