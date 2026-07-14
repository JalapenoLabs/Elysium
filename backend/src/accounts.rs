//! Session account management: the profile and org switchers.
//!
//! Backs the profile dropdown (list signed-in accounts, switch between them, sign
//! one out) and the org switcher (list the active account's orgs, switch active
//! org). Adding an account is not here: it is a normal sign-in performed while a
//! session cookie is present, which [`crate::session::establish`] appends. See
//! `docs/identity.md`.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::cookies;
use crate::session::{self, SessionUser, SESSION_COOKIE};
use crate::state::AppState;

/// `GET /api/v1/session/accounts`: the accounts in this session and the active
/// account's orgs.
pub async fn list(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    jar: CookieJar,
) -> Response {
    let Some(session_id) = session::session_id(&jar) else {
        return StatusCode::UNAUTHORIZED.into_response();
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

    let orgs = sqlx::query_as::<_, (Uuid, String, String, String)>(
        "SELECT o.id, o.name, o.slug, m.role
           FROM sessions s
           JOIN org_members m ON m.user_id = s.active_user_id
           JOIN orgs o ON o.id = m.org_id
          WHERE s.id = $1
          ORDER BY m.joined_at ASC",
    )
    .bind(session_id)
    .fetch_all(&state.pool)
    .await;

    match (accounts, orgs) {
        (Ok(accounts), Ok(orgs)) => Json(json!({
            "accounts": accounts.iter().map(|(id, email, display_name, avatar_url, is_active)| json!({
                "user_id": id,
                "email": email,
                "display_name": display_name,
                "avatar_url": avatar_url,
                "is_active": is_active,
            })).collect::<Vec<_>>(),
            "orgs": orgs.iter().map(|(id, name, slug, role)| json!({
                "id": id,
                "name": name,
                "slug": slug,
                "role": role,
            })).collect::<Vec<_>>(),
            "active_org_id": principal.active_org_id,
        }))
        .into_response(),
        _ => {
            tracing::error!("failed to list session accounts");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// Body of the account-switch request.
#[derive(Deserialize)]
pub struct SwitchAccount {
    user_id: Uuid,
}

/// `POST /api/v1/session/switch`: make another signed-in account active.
pub async fn switch(
    State(state): State<AppState>,
    SessionUser(_principal): SessionUser,
    jar: CookieJar,
    Json(body): Json<SwitchAccount>,
) -> Response {
    let Some(session_id) = session::session_id(&jar) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match session::switch_account(&state.pool, session_id, body.user_id).await {
        // A `false` means the account is not in this session, so it is not switchable.
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "account switch failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// Body of the org-switch request.
#[derive(Deserialize)]
pub struct SwitchOrg {
    org_id: Uuid,
}

/// `POST /api/v1/session/switch-org`: change the active account's active org.
pub async fn switch_org(
    State(state): State<AppState>,
    SessionUser(_principal): SessionUser,
    jar: CookieJar,
    Json(body): Json<SwitchOrg>,
) -> Response {
    let Some(session_id) = session::session_id(&jar) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match session::switch_org(&state.pool, session_id, body.org_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (StatusCode::BAD_REQUEST, "not a member of that org").into_response(),
        Err(error) => {
            tracing::error!(error = %error, "org switch failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// Body of the account sign-out request (defaults to the active account).
#[derive(Deserialize, Default)]
pub struct SignOut {
    #[serde(default)]
    user_id: Option<Uuid>,
}

/// `POST /api/v1/session/signout`: sign one account out of this session.
///
/// Removes the named account (default: the active one). If it was the last
/// account, the session is deleted and the cookie cleared; otherwise another
/// signed-in account becomes active.
pub async fn signout(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    jar: CookieJar,
    Json(body): Json<SignOut>,
) -> Response {
    let Some(session_id) = session::session_id(&jar) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };

    // Default to signing out the active account.
    let target = match body.user_id {
        Some(user_id) => user_id,
        None => match crate::users::id_for_subject(&state.pool, &principal.sub).await {
            Ok(Some(user_id)) => user_id,
            Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
            Err(error) => {
                tracing::error!(error = %error, "active account lookup failed");
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
        },
    };

    match session::remove_account(&state.pool, session_id, target).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        // No accounts remain: the session is gone, so clear the cookie too.
        Ok(false) => (
            jar.remove(cookies::clearing(SESSION_COOKIE)),
            StatusCode::NO_CONTENT,
        )
            .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "account sign-out failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}
