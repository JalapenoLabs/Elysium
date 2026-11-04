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

/// How long Hydra's answer about a token is trusted, unless a revocation drops it first.
const INTROSPECTION_CACHE_TTL: Duration = Duration::from_secs(20);

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
    TOKENS
        .put(
            redis,
            token,
            &introspection,
            INTROSPECTION_CACHE_TTL,
            &owners,
        )
        .await;
    Ok(introspection)
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
