// Copyright © 2026 Jalapeno Labs

//! The access token every MCP request carries, checked before the MCP server sees the request.
//!
//! A token is good when Hydra says it is active, it is an access token, its audience includes
//! the MCP server's own URI (so a token issued for anything else is refused), it grants at least
//! `workspace:read`, and the person it was issued to is still an active, approved user. Each
//! check runs on every request; only Hydra's answer is cached, for [`INTROSPECTION_CACHE_TTL`].
//!
//! A refusal follows the MCP authorization spec: `401` with a `WWW-Authenticate` challenge that
//! names the protected resource metadata, so a client without a token discovers where to get
//! one, and `403 insufficient_scope` for a token that lacks a scope.

use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use redis::AsyncCommands;
use serde_json::json;
use sha2::{Digest, Sha256};
use tracing::{Level, event};
use uuid::Uuid;

use crate::models::user::{self, User};
use crate::oauth::hydra::Introspection;
use crate::oauth::{SCOPE_READ, mcp_resource, mcp_resource_metadata_url};
use crate::state::AppState;

/// How long Hydra's answer about a token is trusted. Also the longest a revoked grant keeps
/// working.
const INTROSPECTION_CACHE_TTL: Duration = Duration::from_secs(20);

/// How stale `users.last_seen_at` may get, as for sessions in the browser.
const LAST_SEEN_GRANULARITY: chrono::Duration = chrono::Duration::minutes(5);

const CACHE_PREFIX: &str = "mcp:token:";

/// Who an MCP request is for, and what their token grants. Inserted for the MCP server, which
/// reaches it through the request's extensions.
#[derive(Debug, Clone)]
pub struct McpCaller {
    pub user: User,
    pub scopes: Vec<String>,
    /// The OAuth client acting for them, such as a Claude Code installation.
    pub client_id: String,
}

/// Why a request was refused, in the terms the challenge uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    /// No `Authorization: Bearer` header at all.
    Missing,
    /// A token Hydra does not vouch for, issued for another resource, or for someone who may not
    /// use Elysium.
    Invalid,
    /// A good token without the scope the request needs.
    InsufficientScope,
}

/// Checks the bearer token and hands the MCP server an [`McpCaller`], or refuses.
pub async fn require_token(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    match caller(&state, request.headers()).await {
        Ok(caller) => {
            request.extensions_mut().insert(caller);
            next.run(request).await
        }
        Err(refusal) => challenge(&state, refusal),
    }
}

async fn caller(state: &AppState, headers: &HeaderMap) -> Result<McpCaller, Refusal> {
    let Some(token) = bearer_token(headers) else {
        return Err(Refusal::Missing);
    };

    let introspection = introspect(state, token).await.ok_or(Refusal::Invalid)?;
    let audience = mcp_resource(&state.auth.public_url);
    let scopes: Vec<String> = introspection
        .scope
        .as_deref()
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let checked = check(&introspection, &audience, &scopes, Utc::now().timestamp())?;

    let mut connection = match state.database.get().await {
        Ok(connection) => connection,
        Err(error) => {
            event!(
                name: "mcp.token.database_unavailable",
                Level::ERROR,
                error.message = %error,
                "no database connection to check an MCP caller",
            );
            return Err(Refusal::Invalid);
        }
    };
    let person = match user::find(&mut connection, checked.user_id).await {
        Ok(person) => person,
        Err(error) => {
            event!(
                name: "mcp.token.unknown_person",
                Level::INFO,
                error.message = %error,
                "refused an MCP token naming no user",
            );
            return Err(Refusal::Invalid);
        }
    };
    if person.kratos_identity_id.is_none() || !person.is_active() {
        event!(
            name: "mcp.token.refused_person",
            Level::INFO,
            user.id = %person.id,
            "refused an MCP token for someone who may not use the workspace",
        );
        return Err(Refusal::Invalid);
    }
    if let Err(error) = user::touch_last_seen(
        &mut connection,
        person.id,
        Utc::now(),
        LAST_SEEN_GRANULARITY,
    )
    .await
    {
        event!(
            name: "mcp.last_seen.failure",
            Level::WARN,
            error.message = %error,
            "could not note an MCP caller as seen",
        );
    }

    Ok(McpCaller {
        user: person,
        scopes,
        client_id: introspection.client_id.unwrap_or_default(),
    })
}

/// What a token Hydra vouched for says, once it passed every check that needs no database.
#[derive(Debug, PartialEq, Eq)]
struct Checked {
    user_id: Uuid,
}

/// Every check on Hydra's answer: active and unexpired, an access token, issued for `audience`,
/// granting `workspace:read`, and naming an Elysium user.
///
/// `now` is seconds since the epoch. Expiry is checked here as well as by Hydra, because a cached
/// answer outlives the moment Hydra gave it: without it, a token would keep working past its own
/// expiry for the rest of the cache window.
fn check(
    introspection: &Introspection,
    audience: &str,
    scopes: &[String],
    now: i64,
) -> Result<Checked, Refusal> {
    let is_access_token = introspection.token_use.as_deref() == Some("access_token");
    let is_for_this_server = introspection.aud.iter().any(|granted| granted == audience);
    let is_unexpired = introspection.exp.is_some_and(|expiry| expiry > now);
    if !introspection.active || !is_access_token || !is_for_this_server || !is_unexpired {
        return Err(Refusal::Invalid);
    }
    if !scopes.iter().any(|scope| scope == SCOPE_READ) {
        return Err(Refusal::InsufficientScope);
    }
    let user_id = introspection
        .sub
        .as_deref()
        .and_then(|subject| Uuid::parse_str(subject).ok())
        .ok_or(Refusal::Invalid)?;
    Ok(Checked { user_id })
}

/// Hydra's answer about `token`, from the cache or from Hydra. `None` when Hydra cannot be
/// reached, which refuses the request: a token Hydra cannot vouch for is no token.
async fn introspect(state: &AppState, token: &str) -> Option<Introspection> {
    let key = format!("{CACHE_PREFIX}{}", hex::encode(Sha256::digest(token)));
    let mut redis = state.redis.clone();
    if let Ok(Some(cached)) = redis.get::<_, Option<String>>(&key).await
        && let Ok(introspection) = serde_json::from_str(&cached)
    {
        return Some(introspection);
    }

    let introspection = match state.auth.hydra.introspect(token).await {
        Ok(introspection) => introspection,
        Err(error) => {
            event!(
                name: "mcp.token.introspection_failure",
                Level::WARN,
                error.message = %error,
                "could not introspect an MCP token",
            );
            return None;
        }
    };
    let serialized = serde_json::to_string(&introspection).expect("an introspection serializes");
    if let Err(error) = redis
        .set_ex::<_, _, ()>(&key, serialized, INTROSPECTION_CACHE_TTL.as_secs())
        .await
    {
        event!(
            name: "mcp.token.cache_failure",
            Level::WARN,
            error.message = %error,
            "could not cache a token introspection",
        );
    }
    Some(introspection)
}

/// The token in `Authorization: Bearer <token>`. The scheme is case-insensitive (RFC 6750).
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    Some(token)
}

/// The refusal: status, `WWW-Authenticate` challenge, and a JSON body saying the same.
fn challenge(state: &AppState, refusal: Refusal) -> Response {
    let metadata = mcp_resource_metadata_url(&state.auth.public_url);
    let (status, error, description) = match refusal {
        Refusal::Missing => (
            StatusCode::UNAUTHORIZED,
            None,
            "an access token is required",
        ),
        Refusal::Invalid => (
            StatusCode::UNAUTHORIZED,
            Some("invalid_token"),
            "the access token is not valid for this server",
        ),
        Refusal::InsufficientScope => (
            StatusCode::FORBIDDEN,
            Some("insufficient_scope"),
            "the access token does not grant workspace:read",
        ),
    };
    let value = match error {
        Some(error) => format!(
            r#"Bearer resource_metadata="{metadata}", scope="{SCOPE_READ}", error="{error}", error_description="{description}""#
        ),
        None => format!(r#"Bearer resource_metadata="{metadata}", scope="{SCOPE_READ}""#),
    };

    let mut response = (
        status,
        axum::Json(json!({ "error": error.unwrap_or("unauthorized"), "message": description })),
    )
        .into_response();
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_str(&value).expect("the challenge is plain ASCII"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUDIENCE: &str = "https://work.example.com/api/mcp";
    const NOW: i64 = 1_790_000_000;

    fn introspection() -> Introspection {
        Introspection {
            active: true,
            sub: Some("0199a3c4-0000-7000-8000-00000000beef".to_owned()),
            scope: Some("workspace:read offline_access".to_owned()),
            aud: vec![AUDIENCE.to_owned()],
            client_id: Some("client".to_owned()),
            exp: Some(NOW + 60),
            token_use: Some("access_token".to_owned()),
        }
    }

    fn scopes(introspection: &Introspection) -> Vec<String> {
        introspection
            .scope
            .as_deref()
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn a_good_token_names_its_user() {
        let good = introspection();
        let checked = check(&good, AUDIENCE, &scopes(&good), NOW).expect("accepted");
        assert_eq!(
            checked.user_id.to_string(),
            "0199a3c4-0000-7000-8000-00000000beef"
        );
    }

    #[test]
    fn tokens_for_another_audience_or_kind_are_refused() {
        let elsewhere = Introspection {
            aud: vec!["https://other.example.com/api/mcp".to_owned()],
            ..introspection()
        };
        assert_eq!(
            check(&elsewhere, AUDIENCE, &scopes(&elsewhere), NOW),
            Err(Refusal::Invalid)
        );

        let unbound = Introspection {
            aud: Vec::new(),
            ..introspection()
        };
        assert_eq!(
            check(&unbound, AUDIENCE, &scopes(&unbound), NOW),
            Err(Refusal::Invalid)
        );

        let refresh = Introspection {
            token_use: Some("refresh_token".to_owned()),
            ..introspection()
        };
        assert_eq!(
            check(&refresh, AUDIENCE, &scopes(&refresh), NOW),
            Err(Refusal::Invalid)
        );

        let revoked = Introspection {
            active: false,
            ..introspection()
        };
        assert_eq!(
            check(&revoked, AUDIENCE, &scopes(&revoked), NOW),
            Err(Refusal::Invalid)
        );
    }

    /// A cached answer once let a token work for up to the cache's lifetime past its own expiry.
    #[test]
    fn an_expired_token_is_refused_even_from_a_cached_answer() {
        let expired = Introspection {
            exp: Some(NOW - 1),
            ..introspection()
        };
        assert_eq!(
            check(&expired, AUDIENCE, &scopes(&expired), NOW),
            Err(Refusal::Invalid)
        );

        let undated = Introspection {
            exp: None,
            ..introspection()
        };
        assert_eq!(
            check(&undated, AUDIENCE, &scopes(&undated), NOW),
            Err(Refusal::Invalid)
        );
    }

    #[test]
    fn a_token_without_read_is_short_of_scope() {
        let write_only = Introspection {
            scope: Some("workspace:write".to_owned()),
            ..introspection()
        };
        assert_eq!(
            check(&write_only, AUDIENCE, &scopes(&write_only), NOW),
            Err(Refusal::InsufficientScope)
        );
    }

    #[test]
    fn a_subject_that_is_not_a_user_id_is_refused() {
        let foreign = Introspection {
            sub: Some("kratos-identity".to_owned()),
            ..introspection()
        };
        assert_eq!(
            check(&foreign, AUDIENCE, &scopes(&foreign), NOW),
            Err(Refusal::Invalid)
        );
    }

    #[test]
    fn reads_bearer_tokens_whatever_the_scheme_case() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer abc"),
        );
        assert_eq!(bearer_token(&headers), Some("abc"));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("bearer abc"),
        );
        assert_eq!(bearer_token(&headers), Some("abc"));
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Basic abc"));
        assert_eq!(bearer_token(&headers), None);
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer "));
        assert_eq!(bearer_token(&headers), None);
    }
}
