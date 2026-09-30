// Copyright © 2026 Jalapeno Labs

//! Whose session a cookie is, asked of Kratos and remembered briefly in Redis.
//!
//! Every API request carries the session cookie, and asking Kratos each time would put a
//! network round trip in front of every one. Kratos's answer is cached for
//! [`SESSION_CACHE_TTL`] under a hash of the cookie, so a session Kratos revokes keeps working
//! for at most that long. Disabling a person is immediate regardless: their `users` row is
//! read on every request. See `docs/auth.md`.
//!
//! Sessions roll: Kratos only extends a session when asked, so the first request after a
//! session is [`EXTEND_AFTER`] old asks, which resets it to a full lifespan.

use std::time::Duration;

use chrono::Utc;
use redis::AsyncCommands;
use redis::aio::ConnectionManager;
use sha2::{Digest, Sha256};
use tracing::{Level, event};

use super::kratos::{Kratos, KratosError, Session};

/// How long a Kratos answer is trusted. Also the longest a revoked session keeps working.
const SESSION_CACHE_TTL: Duration = Duration::from_secs(30);

/// `session.lifespan` in `kratos/kratos.yml`: how long a session lasts from its last
/// extension. Change the two together.
const SESSION_LIFESPAN: chrono::Duration = chrono::Duration::hours(72);

/// How old a session gets before a request extends it: `session.lifespan` less
/// `session.earliest_possible_extend` in `kratos/kratos.yml`. An hour keeps a busy session to
/// one extension write an hour while still meaning "72 hours from the last use".
const EXTEND_AFTER: chrono::Duration = chrono::Duration::hours(1);

const CACHE_PREFIX: &str = "elysium:auth:session:";

/// The session `cookie` belongs to, from the cache or from Kratos.
///
/// # Errors
/// Returns what [`Kratos::whoami`] does. Redis failing is logged and falls back to Kratos.
pub async fn resolve(
    redis: &ConnectionManager,
    kratos: &Kratos,
    cookie: &str,
) -> Result<Session, KratosError> {
    let key = cache_key(cookie);
    let mut redis = redis.clone();

    match redis.get::<_, Option<String>>(&key).await {
        Ok(Some(cached)) => match serde_json::from_str(&cached) {
            Ok(session) => return Ok(session),
            Err(error) => event!(
                name: "auth.session_cache.unreadable",
                Level::WARN,
                error.message = %error,
                "a cached session did not parse; asking Kratos",
            ),
        },
        Ok(None) => {}
        Err(error) => event!(
            name: "auth.session_cache.failure",
            Level::WARN,
            error.message = %error,
            "the session cache is unreachable; asking Kratos",
        ),
    }

    let session = kratos.whoami(cookie).await?;
    let serialized = serde_json::to_string(&session).expect("a session always serializes");
    if let Err(error) = redis
        .set_ex::<_, _, ()>(&key, serialized, SESSION_CACHE_TTL.as_secs())
        .await
    {
        event!(
            name: "auth.session_cache.failure",
            Level::WARN,
            error.message = %error,
            "could not cache a session",
        );
    }
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
    let mut redis = redis.clone();
    if let Err(error) = redis.del::<_, ()>(cache_key(cookie)).await {
        event!(
            name: "auth.session_cache.failure",
            Level::WARN,
            error.message = %error,
            "could not drop an extended session from the cache",
        );
    }
}

/// The cache key for a cookie. Hashed, so Redis never holds a usable session token.
fn cache_key(cookie: &str) -> String {
    format!("{CACHE_PREFIX}{}", hex::encode(Sha256::digest(cookie)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_keys_never_contain_the_cookie() {
        let key = cache_key("ory_st_secret-token-value");
        assert!(key.starts_with(CACHE_PREFIX));
        assert!(!key.contains("secret-token-value"));
        assert_eq!(
            key,
            cache_key("ory_st_secret-token-value"),
            "stable per cookie"
        );
    }
}
