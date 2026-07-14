//! Login history: an append-only audit trail of sign-in attempts.
//!
//! Every sign-in attempt is recorded here, successful or not, so users and
//! admins can audit account access (see `docs/auth-model.md`). Recording is
//! deliberately fire-and-forget for the caller: a failure to write the audit
//! row is logged but never blocks the auth flow itself.

use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::header::USER_AGENT;
use axum::http::request::Parts;
use ipnetwork::IpNetwork;
use sqlx::PgPool;
use uuid::Uuid;

/// How the sign-in was attempted. Grows with the auth roadmap.
#[derive(Debug, Clone, Copy)]
pub enum Method {
    Passkey,
    Password,
    Totp,
    Recovery,
}

impl Method {
    /// The stable identifier persisted in `login_events.method`.
    ///
    /// Also the value the rate limiter (see [`crate::rate_limit`]) filters on,
    /// so it counts failures of the same method it is guarding.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passkey => "passkey",
            Self::Password => "password",
            Self::Totp => "totp",
            Self::Recovery => "recovery",
        }
    }
}

/// What came of the attempt.
#[derive(Debug, Clone, Copy)]
pub enum Outcome {
    Success,
    Failure,
}

impl Outcome {
    /// The stable identifier persisted in `login_events.outcome`.
    fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
        }
    }
}

/// The requesting client, captured for the audit trail.
///
/// Extracted from request parts: the IP prefers the nearest proxy hop
/// (`X-Forwarded-For`, set by the SPA proxy or the operator's gateway) and falls
/// back to the socket peer address. Extraction is infallible; missing pieces
/// are recorded as absent rather than failing the request.
#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub ip: Option<IpAddr>,
    pub user_agent: String,
}

impl<S> FromRequestParts<S> for ClientInfo
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let forwarded = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .and_then(|first_hop| first_hop.trim().parse().ok());
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(address)| address.ip());
        let user_agent = parts
            .headers
            .get(USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();

        Ok(Self {
            ip: forwarded.or(peer),
            user_agent,
        })
    }
}

/// Records one sign-in attempt; logs (never propagates) a write failure.
///
/// `user_id` is `None` when the attempt never resolved to a user (an unknown
/// email). `email` may be empty when the step provides none (a ceremony finish
/// identifies the user by ceremony state instead).
pub async fn record(
    pool: &PgPool,
    user_id: Option<Uuid>,
    email: &str,
    method: Method,
    outcome: Outcome,
    client: &ClientInfo,
) {
    let result = sqlx::query(
        "INSERT INTO login_events (user_id, email, method, outcome, ip_address, user_agent)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(user_id)
    .bind(email)
    .bind(method.as_str())
    .bind(outcome.as_str())
    .bind(client.ip.map(IpNetwork::from))
    .bind(&client.user_agent)
    .execute(pool)
    .await;

    if let Err(error) = result {
        tracing::error!(error = %error, "failed to record login event");
    }
}
