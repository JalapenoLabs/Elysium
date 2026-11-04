// Copyright © 2026 Jalapeno Labs

//! Whose session a cookie is, asked of Kratos and cached in Redis, and every way a session ends.
//!
//! Every API request carries the session cookie, and asking Kratos each time would put a
//! network round trip in front of every one. Kratos's answer is cached for
//! [`SESSION_CACHE_TTL`] under a hash of the cookie, filed under the person's Kratos identity
//! ([`super::revocable_cache`]). Disabling a person is immediate regardless: their `users` row is
//! read on every request. See `docs/auth.md`.
//!
//! A session ends only through this module, so the cache always hears of it: signing out,
//! ending one's own other sessions, an admin signing someone out everywhere, deleting an
//! identity, and Kratos's own revocations on a settings change, which its webhook reports
//! (`crate::routes::internal`). Each asks Kratos first and then forgets every session the person
//! has cached, so the ended one is refused on its very next request. The person's other sessions
//! simply ask Kratos again once.
//!
//! Sessions roll: Kratos only extends a session when asked, so the first request after a
//! session is [`EXTEND_AFTER`] old asks, which resets it to a full lifespan.

use std::time::Duration;

use chrono::Utc;
use redis::aio::ConnectionManager;
use reqwest::header::HeaderValue;
use tracing::{Level, event};
use uuid::Uuid;

use super::kratos::{self, Kratos, KratosError, Session};
use super::revocable_cache::{Owner, REVOCATION_GRACE, RevocableCache};

// A revocation must keep answers out for longer than a `whoami` in flight can take to return
// one from before it.
const _: () = assert!(REVOCATION_GRACE.as_secs() > kratos::REQUEST_TIMEOUT.as_secs());

/// How long a Kratos answer is trusted, unless a revocation drops it first.
const SESSION_CACHE_TTL: Duration = Duration::from_secs(30);

/// `session.lifespan` in `kratos/kratos.yml`: how long a session lasts from its last
/// extension. Change the two together.
const SESSION_LIFESPAN: chrono::Duration = chrono::Duration::hours(72);

/// How old a session gets before a request extends it: `session.lifespan` less
/// `session.earliest_possible_extend` in `kratos/kratos.yml`. An hour keeps a busy session to
/// one extension write an hour while still meaning "72 hours from the last use".
const EXTEND_AFTER: chrono::Duration = chrono::Duration::hours(1);

/// Kratos's answers, each filed under the identity whose session it is.
const SESSIONS: RevocableCache = RevocableCache::new("elysium:auth:sessions");

/// The session `cookie` belongs to, from the cache or from Kratos.
///
/// # Errors
/// Returns what [`Kratos::whoami`] does. Redis failing is logged and falls back to Kratos.
pub async fn resolve(
    redis: &ConnectionManager,
    kratos: &Kratos,
    cookie: &str,
) -> Result<Session, KratosError> {
    if let Some(session) = SESSIONS.get(redis, cookie).await {
        return Ok(session);
    }

    let session = kratos.whoami(cookie).await?;
    SESSIONS
        .put(
            redis,
            cookie,
            &session,
            SESSION_CACHE_TTL,
            &[person(session.identity.id)],
        )
        .await;
    Ok(session)
}

/// Extends `session` when it is due, and drops the cached copy so the next request sees the
/// new expiry. A failure is logged, never fatal: the session is still valid as it stands.
pub async fn extend_if_due(
    redis: &ConnectionManager,
    kratos: &Kratos,
    cookie: &str,
    session: &Session,
) {
    let remaining = session.expires_at - Utc::now();
    if remaining > SESSION_LIFESPAN - EXTEND_AFTER {
        return;
    }

    if let Err(error) = kratos.extend_session(session.id).await {
        event!(
            name: "auth.session.extend_failure",
            Level::WARN,
            error.message = %error,
            "could not extend a session",
        );
        return;
    }
    SESSIONS.remove(redis, cookie).await;
}

/// Signs the session `cookie` holds out, and answers the `Set-Cookie` headers Kratos sent to
/// clear the cookie in the browser. A session still waiting on its second factor signs out too.
///
/// # Errors
/// Returns [`KratosError::NoSession`] when the cookie holds no session, and what else
/// [`Kratos`] does when it cannot sign it out.
pub async fn sign_out(
    redis: &ConnectionManager,
    kratos: &Kratos,
    cookie: &str,
) -> Result<Vec<HeaderValue>, KratosError> {
    // Whose session it is, asked while it still exists. One waiting on its second factor has no
    // answer to give, and Kratos never answered for it, so nothing of it was cached. Any other
    // failure is Kratos's, which the logout reports in turn.
    let identity = resolve(redis, kratos, cookie)
        .await
        .ok()
        .map(|session| session.identity.id);
    let clearing = kratos.log_out(cookie).await?;
    if let Some(identity) = identity {
        forget(redis, identity).await;
    }
    Ok(clearing)
}

/// Ends `session_id`, one of `identity`'s other sessions, at the request of the session `cookie`
/// holds, which Kratos checks is the same person's.
///
/// # Errors
/// Returns [`KratosError::NotFound`] for a session that is not theirs.
pub async fn end_session(
    redis: &ConnectionManager,
    kratos: &Kratos,
    cookie: &str,
    identity: Uuid,
    session_id: Uuid,
) -> Result<(), KratosError> {
    kratos.revoke_own_session(cookie, session_id).await?;
    forget(redis, identity).await;
    Ok(())
}

/// Ends every session of `identity` but the one `cookie` holds.
///
/// # Errors
/// Returns what [`Kratos`] does when it cannot end them.
pub async fn end_other_sessions(
    redis: &ConnectionManager,
    kratos: &Kratos,
    cookie: &str,
    identity: Uuid,
) -> Result<(), KratosError> {
    kratos.revoke_own_other_sessions(cookie).await?;
    forget(redis, identity).await;
    Ok(())
}

/// Signs `identity` out everywhere, for an admin.
///
/// # Errors
/// Returns [`KratosError::Refused`] when Kratos cannot be reached.
pub async fn end_all_sessions(
    redis: &ConnectionManager,
    kratos: &Kratos,
    identity: Uuid,
) -> Result<(), KratosError> {
    kratos.revoke_sessions(identity).await?;
    forget(redis, identity).await;
    Ok(())
}

/// Deletes `identity` from Kratos, with its credentials and sessions.
///
/// # Errors
/// Returns [`KratosError::Refused`] when Kratos cannot be reached.
pub async fn delete_identity(
    redis: &ConnectionManager,
    kratos: &Kratos,
    identity: Uuid,
) -> Result<(), KratosError> {
    kratos.delete_identity(identity).await?;
    forget(redis, identity).await;
    Ok(())
}

/// Forgets every session of `identity` that is cached, so each asks Kratos again on its next
/// request. For what Kratos changed on its own, such as revoking the other sessions when a
/// password changes; everything else here calls it once Kratos has answered.
///
/// It runs after Kratos's change, never before: forgetting first would let a request in between
/// cache Kratos's answer from before the change. A failure is logged, not answered (see
/// [`RevocableCache::revoke`]).
pub async fn forget(redis: &ConnectionManager, identity: Uuid) {
    SESSIONS.revoke(redis, &person(identity)).await;
}

/// Caches `session` for `cookie` as if Kratos had just answered for it, so router tests sign
/// in without a Kratos. Kept for ten minutes rather than [`SESSION_CACHE_TTL`], so no test
/// outlives its session.
#[cfg(test)]
pub async fn remember(redis: &ConnectionManager, cookie: &str, session: &Session) {
    SESSIONS
        .put(
            redis,
            cookie,
            session,
            Duration::from_mins(10),
            &[person(session.identity.id)],
        )
        .await;
}

/// What a cached session is filed under: the Kratos identity it signs in.
fn person(identity: Uuid) -> Owner {
    Owner::new("identity", &identity)
}
