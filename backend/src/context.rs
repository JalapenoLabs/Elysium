//! The authenticated session bootstrap: `GET /api/v1/context`.
//!
//! One request returns everything a client needs to render and gate itself after
//! sign-in or a refresh: the active profile, every account in the session, the
//! active account's orgs, and the realtime high-water mark. Aggregating these into
//! one response lets the backend run the reads in parallel and gives the client a
//! single gate instead of a fan of probes.
//!
//! The `as_of_sequence` is the reconciliation hinge (see `docs/realtime.md`): the
//! client connects the SSE stream first, fetches this context second, then applies
//! only SSE events whose sequence exceeds `as_of_sequence`. That closes the gap
//! where an event could land between the snapshot read and the live subscription.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::CookieJar;
use chrono::Utc;
use uuid::Uuid;

use crate::profile;
use crate::proto::common::v1::{Timestamp, Uuid as ProtoUuid};
use crate::proto::session::v1::{SessionAccount, SessionContext, SessionOrg};
use crate::protobuf::Protobuf;
use crate::session::{self, SessionUser};
use crate::state::AppState;

/// `GET /api/v1/context`: the full session context for the active account.
///
/// Protected by the session cookie. Every field is scoped to the caller's own
/// session and active account, so it never exposes another session's accounts or
/// an org the active account is not a member of.
pub async fn context(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    jar: CookieJar,
) -> Response {
    let Some(session_id) = session::session_id(&jar) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };

    let user = match profile::fetch(&state, &principal.sub).await {
        Ok(Some(user)) => user,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "context profile load failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let accounts = sqlx::query_as::<_, (Uuid, String, String, String, bool)>(
        "SELECT u.id, u.email, u.display_name, u.avatar_url, (u.id = s.active_user_id) AS is_active
           FROM session_accounts sa
           JOIN users u ON u.id = sa.user_id
           JOIN sessions s ON s.id = sa.session_id
          WHERE sa.session_id = $1
          ORDER BY sa.added_at ASC",
    )
    .bind(session_id)
    .fetch_all(&state.pool)
    .await;

    let orgs = sqlx::query_as::<_, (Uuid, String, String, String, String, String, String)>(
        "SELECT o.id, o.name, o.slug, m.role, o.logo_url, o.accent_color, o.wordmark
           FROM sessions s
           JOIN org_members m ON m.user_id = s.active_user_id
           JOIN orgs o ON o.id = m.org_id
          WHERE s.id = $1
          ORDER BY m.joined_at ASC",
    )
    .bind(session_id)
    .fetch_all(&state.pool)
    .await;

    let (accounts, orgs) = match (accounts, orgs) {
        (Ok(accounts), Ok(orgs)) => (accounts, orgs),
        _ => {
            tracing::error!("failed to load session context");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    // Read the realtime high-water AFTER loading state: any event still in flight
    // then reaches the client with a later sequence and is applied on top, so the
    // snapshot is never double-applied and never misses a change.
    let as_of_sequence = state.sse.sequence(&principal.sub);

    // Whether the caller must enroll a second factor before using the app: they
    // belong to an org that mandates TOTP and have none confirmed yet.
    let totp_enrollment_required = match mandatory_totp_missing(&state, &principal.sub).await {
        Ok(required) => required,
        Err(error) => {
            tracing::error!(error = %error, "totp requirement check failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    // Whether the caller's active-org membership is suspended, so the client can
    // route them to the suspended screen without waiting for a 403.
    let active_org_suspended =
        match active_org_suspended(&state, &principal.sub, principal.active_org_id).await {
            Ok(suspended) => suspended,
            Err(error) => {
                tracing::error!(error = %error, "active-org suspension check failed");
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
        };

    let context = SessionContext {
        user: Some(user),
        accounts: accounts
            .into_iter()
            .map(
                |(id, email, display_name, avatar_url, is_active)| SessionAccount {
                    user_id: Some(proto_uuid(id)),
                    email,
                    display_name,
                    avatar_url,
                    is_active,
                },
            )
            .collect(),
        orgs: orgs
            .into_iter()
            .map(
                |(id, name, slug, role, logo_url, accent_color, wordmark)| SessionOrg {
                    id: Some(proto_uuid(id)),
                    name,
                    slug,
                    role,
                    logo_url,
                    accent_color,
                    wordmark,
                },
            )
            .collect(),
        active_org_id: principal.active_org_id.map(proto_uuid),
        as_of_sequence,
        server_time: Some(now()),
        totp_enrollment_required,
        active_org_suspended,
    };

    Protobuf(context).into_response()
}

/// Whether the caller's active-org membership is suspended.
///
/// # Errors
/// Returns an error if a query fails.
async fn active_org_suspended(
    state: &AppState,
    subject: &str,
    active_org_id: Option<Uuid>,
) -> sqlx::Result<bool> {
    let Some(org_id) = active_org_id else {
        return Ok(false);
    };
    let Some(user_id) = crate::users::id_for_subject(&state.pool, subject).await? else {
        return Ok(false);
    };
    Ok(crate::authz::membership(&state.pool, user_id, org_id)
        .await?
        .is_some_and(|entry| entry.suspended))
}

/// Whether the caller is in a `require_totp` org but has no confirmed TOTP.
///
/// TOTP is one factor per profile, so any mandating org means the profile must
/// have it; an unknown subject reads as not required. Shared with the OIDC
/// authorize endpoint, which must enforce the same 2FA policy before it issues a
/// code (see [`crate::oidc`]).
///
/// # Errors
/// Returns an error if a query fails.
pub(crate) async fn mandatory_totp_missing(state: &AppState, subject: &str) -> sqlx::Result<bool> {
    let Some(user_id) = crate::users::id_for_subject(&state.pool, subject).await? else {
        return Ok(false);
    };
    let mandated = sqlx::query_scalar::<_, bool>(
        "SELECT COALESCE(bool_or(p.require_totp), false)
           FROM org_members m
           JOIN org_auth_policies p ON p.org_id = m.org_id
          WHERE m.user_id = $1",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    if !mandated {
        return Ok(false);
    }
    Ok(!crate::users::has_totp(&state.pool, user_id).await?)
}

/// Wraps a database `Uuid` in the proto string form.
fn proto_uuid(id: Uuid) -> ProtoUuid {
    ProtoUuid {
        value: id.to_string(),
    }
}

/// The current instant as a proto `Timestamp` (UTC).
fn now() -> Timestamp {
    let instant = Utc::now();
    Timestamp {
        epoch_seconds: instant.timestamp(),
        nanos: instant.timestamp_subsec_nanos(),
        timezone: "Etc/UTC".to_string(),
    }
}
