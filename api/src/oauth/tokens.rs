// Copyright © 2026 Jalapeno Labs

//! What Hydra says about an MCP client's access token, cached in Redis, and every way a grant
//! ends.
//!
//! The MCP server checks the token on every request (`crate::mcp::bearer`), and asking Hydra
//! each time would put a round trip in front of every tool call. Hydra's answer is cached for
//! [`INTROSPECTION_CACHE_TTL`] under a hash of the token, filed under the person it was issued to
//! and the client holding it ([`crate::auth::revocable_cache`]).
//!
//! A grant ends only through this module, so the cache always hears of it: a person
//! disconnecting a client, an admin deleting a client, and everything that signs a person out
//! everywhere. Each tells Hydra first, which revokes the tokens, and then forgets the tokens
//! cached under the person or the client, so each is refused on its very next request.

use std::time::Duration;

use redis::aio::ConnectionManager;
use tracing::{Level, event};
use uuid::Uuid;

use super::hydra::{self, Hydra, HydraError, Introspection};
use crate::auth::revocable_cache::{Owner, REVOCATION_GRACE, RevocableCache};

// A revocation must keep answers out for longer than an introspection in flight can take to
// return one from before it.
const _: () = assert!(REVOCATION_GRACE.as_secs() > hydra::REQUEST_TIMEOUT.as_secs());

/// How long Hydra's answer about a token is trusted, unless a revocation drops it first, and never
/// past the token's own expiry ([`cache_lifetime`]).
///
/// Every revocation Elysium makes forgets the answer at once, so this only bounds what Hydra does
/// without telling it: a client revoking its own token (`/oauth2/revoke`) or replacing it by
/// refreshing. Only the client holding the token can do either, so a few minutes is safe; it is
/// kept shorter than the session cache's because an access token lives an hour at most.
const INTROSPECTION_CACHE_TTL: Duration = Duration::from_mins(5);

/// Hydra's answers, each filed under the person and the client the token was issued to.
const TOKENS: RevocableCache = RevocableCache::new("elysium:mcp:tokens");

/// Hydra's answer about `token`, from the cache or from Hydra.
///
/// # Errors
/// Returns [`HydraError::Refused`] when Hydra cannot be reached. Redis failing is logged and
/// falls back to Hydra.
pub async fn introspect(
    redis: &ConnectionManager,
    hydra: &Hydra,
    token: &str,
) -> Result<Introspection, HydraError> {
    if let Some(introspection) = TOKENS.get(redis, token).await {
        return Ok(introspection);
    }

    let introspection = hydra.introspect(token).await?;
    let Some(owners) = owners(&introspection) else {
        // Hydra issues tokens only through Elysium's consent, which always names both, so this
        // is never expected. Left uncached, revoking still reaches the token at once.
        event!(
            name: "mcp.token.unowned",
            Level::WARN,
            "Hydra vouched for a token without naming its person and client; not caching it",
        );
        return Ok(introspection);
    };
    let lifetime = cache_lifetime(&introspection, chrono::Utc::now().timestamp());
    TOKENS
        .put(redis, token, &introspection, lifetime, &owners)
        .await;
    Ok(introspection)
}

/// How long `introspection` may stay cached from `now`, seconds since the epoch:
/// [`INTROSPECTION_CACHE_TTL`], or less when an active token expires sooner, so an answer never
/// outlives its token. An inactive token never becomes active again, so its answer is kept the
/// whole time.
fn cache_lifetime(introspection: &Introspection, now: i64) -> Duration {
    if !introspection.active {
        return INTROSPECTION_CACHE_TTL;
    }
    let remaining = introspection
        .exp
        .and_then(|expiry| u64::try_from(expiry - now).ok())
        .map_or(Duration::ZERO, Duration::from_secs);
    remaining.min(INTROSPECTION_CACHE_TTL)
}

/// Disconnects `client_id` from `subject`, the person who connected it, revoking every token it
/// holds for them.
///
/// # Errors
/// Returns [`HydraError::Refused`] when Hydra cannot be reached.
pub async fn revoke_grant(
    redis: &ConnectionManager,
    hydra: &Hydra,
    subject: Uuid,
    client_id: &str,
) -> Result<(), HydraError> {
    hydra.revoke_consent(subject, Some(client_id)).await?;
    // Every token of the person, not only this client's: the others simply ask Hydra again.
    TOKENS.revoke(redis, &person(subject)).await;
    Ok(())
}

/// Disconnects every client `subject` connected, as signing them out everywhere does.
///
/// # Errors
/// Returns [`HydraError::Refused`] when Hydra cannot be reached.
pub async fn revoke_all_grants(
    redis: &ConnectionManager,
    hydra: &Hydra,
    subject: Uuid,
) -> Result<(), HydraError> {
    hydra.revoke_consent(subject, None).await?;
    TOKENS.revoke(redis, &person(subject)).await;
    Ok(())
}

/// Deletes the client `client_id` with every grant and token it holds, for everyone.
///
/// # Errors
/// Returns [`HydraError::NotFound`] for an unknown client.
pub async fn delete_client(
    redis: &ConnectionManager,
    hydra: &Hydra,
    client_id: &str,
) -> Result<(), HydraError> {
    hydra.delete_client(client_id).await?;
    TOKENS.revoke(redis, &client(client_id)).await;
    Ok(())
}

/// What an answer is filed under: the person and the client it was issued to. An inactive answer
/// is filed under nothing, since a token never becomes active again and so needs no revoking.
/// `None` for an active answer that does not name both, which no revocation could then find.
fn owners(introspection: &Introspection) -> Option<Vec<Owner>> {
    if !introspection.active {
        return Some(Vec::new());
    }
    let subject = introspection
        .sub
        .as_deref()
        .and_then(|subject| Uuid::parse_str(subject).ok())?;
    let client_id = introspection.client_id.as_deref()?;
    Some(vec![person(subject), client(client_id)])
}

fn person(subject: Uuid) -> Owner {
    Owner::new("subject", &subject)
}

fn client(client_id: &str) -> Owner {
    Owner::new("client", &client_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_790_000_000;

    fn active_until(exp: Option<i64>) -> Introspection {
        Introspection {
            active: true,
            sub: Some(Uuid::now_v7().to_string()),
            scope: Some("workspace:read".to_owned()),
            aud: Vec::new(),
            client_id: Some("client".to_owned()),
            exp,
            token_use: Some("access_token".to_owned()),
        }
    }

    #[test]
    fn a_token_is_cached_for_the_full_lifetime_when_it_lasts_longer() {
        let token = active_until(Some(NOW + 3600));
        assert_eq!(cache_lifetime(&token, NOW), INTROSPECTION_CACHE_TTL);
    }

    /// #190: a longer cache must never let an answer outlive its token.
    #[test]
    fn a_token_about_to_expire_is_cached_only_until_it_does() {
        let token = active_until(Some(NOW + 90));
        assert_eq!(cache_lifetime(&token, NOW), Duration::from_secs(90));
    }

    #[test]
    fn an_expired_or_undated_token_is_not_cached() {
        assert_eq!(
            cache_lifetime(&active_until(Some(NOW - 1)), NOW),
            Duration::ZERO
        );
        assert_eq!(cache_lifetime(&active_until(None), NOW), Duration::ZERO);
    }

    #[test]
    fn an_inactive_answer_is_kept_the_whole_time() {
        let revoked = Introspection {
            active: false,
            ..active_until(None)
        };
        assert_eq!(cache_lifetime(&revoked, NOW), INTROSPECTION_CACHE_TTL);
    }

    #[test]
    fn only_an_active_answer_naming_its_person_and_client_is_filed() {
        assert_eq!(
            owners(&active_until(Some(NOW + 60))).map(|owners| owners.len()),
            Some(2)
        );
        let anonymous = Introspection {
            sub: None,
            ..active_until(Some(NOW + 60))
        };
        assert_eq!(owners(&anonymous), None);
        let revoked = Introspection {
            active: false,
            ..anonymous
        };
        assert_eq!(owners(&revoked), Some(Vec::new()));
    }
}
