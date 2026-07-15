//! Invites: single-use, expiring links that onboard a new member.
//!
//! An existing member mints an invite for their org; the plaintext link token is
//! returned once and only its hash is stored, so the copyable link is the secret
//! (no SMTP needed). The invitee previews it (public), then accepts by enrolling a
//! credential exactly like first-run claim, reusing [`crate::enrollment`]. Accept
//! is atomic: the user, membership, recovery codes, and session are written, and
//! the invite is marked consumed, in one transaction. See `docs/auth.md`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use chrono::{Duration, Utc};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::audit;
use crate::authz;
use crate::crypto::Crypto;
use crate::enrollment::{self, FinishBody, PendingCredential};
use crate::login_events::{self, ClientInfo};
use crate::session::{self, SessionUser, SESSION_COOKIE};
use crate::state::AppState;
use crate::{cookies, recovery, users};

/// Short-lived cookie carrying the in-flight invite acceptance between begin/finish.
const INVITE_COOKIE: &str = "elysium_invite";

/// How long a freshly minted invite stays valid.
const INVITE_LIFETIME: Duration = Duration::days(7);

/// Random bytes in an invite token (256 bits: unguessable).
const TOKEN_BYTES: usize = 32;

/// Body of the mint request. All fields optional; sensible defaults apply.
#[derive(Deserialize, Default)]
pub struct MintRequest {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    team_id: Option<Uuid>,
    #[serde(default)]
    email: Option<String>,
}

/// `POST /api/v1/auth/invites`: mint an invite link for the caller's org.
///
/// Inviting is an owner/admin capability, and the invited role may not exceed the
/// inviter's own, so an admin cannot mint an owner invite and escalate. Returns the
/// one-time link token and its acceptance URL.
pub async fn mint(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Json(body): Json<MintRequest>,
) -> Response {
    // Invites target the inviter's active org.
    let Some(org_id) = principal.active_org_id else {
        return (StatusCode::BAD_REQUEST, "no active org").into_response();
    };
    let created_by = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "inviter lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    // Only an owner or admin of the active org may invite. Reuse `created_by` so we
    // resolve the caller once rather than through the extractor a second time.
    let inviter_role = match authz::role_in_org(&state.pool, created_by, org_id).await {
        Ok(Some(role)) if role >= authz::OrgRole::Admin => role,
        Ok(_) => return StatusCode::FORBIDDEN.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "inviter role lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let role = match body.role.as_deref().unwrap_or("member") {
        role @ ("guest" | "member" | "admin" | "owner") => role,
        _ => return (StatusCode::BAD_REQUEST, "invalid role").into_response(),
    };
    // No privilege escalation: the invited role cannot outrank the inviter.
    if authz::OrgRole::from_db(role).is_some_and(|requested| requested > inviter_role) {
        return (StatusCode::FORBIDDEN, "cannot invite above your own role").into_response();
    }

    // A team, if given, must belong to the caller's org.
    if let Some(team_id) = body.team_id {
        match team_in_org(&state.pool, team_id, org_id).await {
            Ok(true) => {}
            Ok(false) => return (StatusCode::BAD_REQUEST, "unknown team").into_response(),
            Err(error) => {
                tracing::error!(error = %error, "team check failed");
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
        }
    }

    let token = mint_token();
    let expires_at = Utc::now() + INVITE_LIFETIME;
    let insert = sqlx::query(
        "INSERT INTO invites (org_id, team_id, role, email, token_hash, created_by, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(org_id)
    .bind(body.team_id)
    .bind(role)
    .bind(body.email.as_deref().unwrap_or("").trim())
    .bind(hash(&token))
    .bind(created_by)
    .bind(expires_at)
    .execute(&state.pool)
    .await;

    if let Err(error) = insert {
        tracing::error!(error = %error, "invite insert failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    audit::record(
        &state.pool,
        org_id,
        created_by,
        "invite.created",
        None,
        json!({ "role": role, "email": body.email.as_deref().unwrap_or("").trim() }),
    )
    .await;
    Json(json!({
        "token": token,
        "url": format!("{}/invite/{}", state.app_origin, token),
        "role": role,
        "expires_at": expires_at.to_rfc3339(),
    }))
    .into_response()
}

/// A valid invite resolved from a token, with its org (and optional team) names.
struct Resolved {
    id: Uuid,
    org_id: Uuid,
    org_name: String,
    team_id: Option<Uuid>,
    team_name: Option<String>,
    role: String,
    email: String,
}

/// `GET /api/v1/auth/invites/:token`: preview an invite before accepting. Public.
///
/// A missing, expired, or already-accepted token is a generic `404`, so the
/// endpoint does not distinguish "never existed" from "already used".
pub async fn preview(State(state): State<AppState>, Path(token): Path<String>) -> Response {
    match resolve(&state.pool, &token).await {
        Ok(Some(invite)) => Json(json!({
            "org_name": invite.org_name,
            "team_name": invite.team_name,
            "role": invite.role,
            "email": invite.email,
        }))
        .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "invite preview failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// The in-flight invite acceptance, serialized into the cookie.
#[derive(Serialize, Deserialize)]
struct Pending {
    invite_id: Uuid,
    org_id: Uuid,
    team_id: Option<Uuid>,
    role: String,
    user_id: Uuid,
    email: String,
    given_name: String,
    family_name: String,
    display_name: String,
    credential: PendingCredential,
}

/// Body of the accept-begin request: the invitee's identity and chosen method.
#[derive(Deserialize)]
pub struct AcceptBegin {
    email: String,
    #[serde(default)]
    given_name: String,
    #[serde(default)]
    family_name: String,
    #[serde(default)]
    display_name: String,
    method: String,
    opaque_registration_request: Option<String>,
}

/// `POST /api/v1/auth/invites/:token/accept/begin`: start accepting an invite.
///
/// Persists nothing; mirrors bootstrap begin but keyed by the invite token.
pub async fn accept_begin(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(token): Path<String>,
    Json(body): Json<AcceptBegin>,
) -> Response {
    let invite = match resolve(&state.pool, &token).await {
        Ok(Some(invite)) => invite,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "invite lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let user_id = Uuid::new_v4();
    let display_name = match (body.display_name.trim(), body.given_name.trim()) {
        ("", "") => body.email.clone(),
        ("", given) => given.to_string(),
        (chosen, _) => chosen.to_string(),
    };

    let (credential, payload) = match enrollment::begin(
        &state,
        user_id,
        &body.email,
        &display_name,
        &body.method,
        body.opaque_registration_request.as_deref(),
    ) {
        Ok(pair) => pair,
        Err(response) => return response,
    };

    let pending = Pending {
        invite_id: invite.id,
        org_id: invite.org_id,
        team_id: invite.team_id,
        role: invite.role,
        user_id,
        email: body.email,
        given_name: body.given_name.trim().to_string(),
        family_name: body.family_name.trim().to_string(),
        display_name,
        credential,
    };
    let cookie = cookies::build(
        INVITE_COOKIE,
        serde_json::to_string(&pending).expect("pending invite serializes"),
    );
    (jar.add(cookie), Json(payload)).into_response()
}

/// `POST /api/v1/auth/invites/accept/finish`: verify the credential and join.
///
/// Cookie-driven (like bootstrap finish). Atomically creates the user, their
/// membership, recovery codes, and session, and marks the invite consumed. On
/// success returns `200` with the one-time recovery codes and the session cookie.
pub async fn accept_finish(
    State(state): State<AppState>,
    jar: CookieJar,
    client: ClientInfo,
    Json(body): Json<FinishBody>,
) -> Response {
    let Some(pending) = read_pending(&jar) else {
        return (StatusCode::BAD_REQUEST, "no invite in progress").into_response();
    };
    let jar = jar.remove(cookies::clearing(INVITE_COOKIE));

    let credential = match enrollment::verify(&state, &pending.credential, &body) {
        Ok(credential) => credential,
        Err(response) => return response,
    };
    let method = credential.method();

    let codes = recovery::generate();
    let session_id = match join(
        &state.pool,
        &state.crypto,
        &client,
        &pending,
        &credential,
        &codes,
    )
    .await
    {
        Ok(Some(session_id)) => session_id,
        // The invite was consumed or expired between begin and finish.
        Ok(None) => return (StatusCode::CONFLICT, "invite is no longer valid").into_response(),
        Err(error) if is_duplicate_email(&error) => {
            // The email already has a profile: this is a join, not a new signup.
            // Tell the client to sign in and use the authenticated join instead.
            return (StatusCode::CONFLICT, "account exists").into_response();
        }
        Err(error) => {
            tracing::error!(error = %error, "invite acceptance failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    login_events::record(
        &state.pool,
        Some(pending.user_id),
        &pending.email,
        method,
        login_events::Outcome::Success,
        &client,
    )
    .await;

    let cookie = cookies::build(SESSION_COOKIE, session_id.to_string());
    (jar.add(cookie), Json(json!({ "recovery_codes": codes }))).into_response()
}

/// `POST /api/v1/auth/invites/:token/accept/join`: an existing, signed-in profile
/// joins the invite's org (multi-org). No enrollment: the caller already has an
/// account, so this just adds the membership and consumes the invite.
pub async fn accept_join(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(token): Path<String>,
) -> Response {
    let invite = match resolve(&state.pool, &token).await {
        Ok(Some(invite)) => invite,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "invite lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    match join_existing(&state.pool, &invite, user_id).await {
        Ok(true) => {
            Json(json!({ "org_id": invite.org_id, "org_name": invite.org_name })).into_response()
        }
        Ok(false) => (StatusCode::CONFLICT, "invite is no longer valid").into_response(),
        Err(error) => {
            tracing::error!(error = %error, "invite join failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Consumes the invite and adds an existing profile to its org, atomically.
///
/// Returns `Ok(false)` if the invite was already used or expired.
///
/// # Errors
/// Returns an error if any write fails.
async fn join_existing(pool: &PgPool, invite: &Resolved, user_id: Uuid) -> sqlx::Result<bool> {
    let mut tx = pool.begin().await?;
    let consumed = sqlx::query(
        "UPDATE invites SET accepted_at = now()
          WHERE id = $1 AND accepted_at IS NULL AND expires_at > now()",
    )
    .bind(invite.id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if consumed == 0 {
        return Ok(false);
    }

    users::add_membership(&mut tx, user_id, invite.org_id, &invite.role).await?;
    if let Some(team_id) = invite.team_id {
        sqlx::query(
            "INSERT INTO team_members (user_id, team_id, role) VALUES ($1, $2, $3)
             ON CONFLICT (user_id, team_id) DO NOTHING",
        )
        .bind(user_id)
        .bind(team_id)
        .bind(&invite.role)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(true)
}

/// Whether a database error is a unique-violation on the email index.
fn is_duplicate_email(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
}

/// Claims the invite and provisions the member in one transaction.
///
/// Returns `Ok(None)` if the invite was consumed or expired between begin and
/// finish (the marking `UPDATE` matches no row), so acceptance is single-use even
/// under a race.
///
/// # Errors
/// Returns an error if any insert fails.
async fn join(
    pool: &PgPool,
    crypto: &Crypto,
    client: &ClientInfo,
    pending: &Pending,
    credential: &enrollment::Credential,
    recovery_codes: &[String],
) -> sqlx::Result<Option<Uuid>> {
    let mut tx = pool.begin().await?;

    // Consume the invite first; a no-op means it is already used or expired.
    let consumed = sqlx::query(
        "UPDATE invites SET accepted_at = now()
          WHERE id = $1 AND accepted_at IS NULL AND expires_at > now()",
    )
    .bind(pending.invite_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if consumed == 0 {
        return Ok(None);
    }

    // A new, org-independent profile; the org membership is added below.
    sqlx::query(
        "INSERT INTO users (id, subject, email, given_name, family_name, display_name)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(pending.user_id)
    .bind(Uuid::new_v4().to_string())
    .bind(&pending.email)
    .bind(&pending.given_name)
    .bind(&pending.family_name)
    .bind(&pending.display_name)
    .execute(&mut *tx)
    .await?;

    users::add_membership(&mut tx, pending.user_id, pending.org_id, &pending.role).await?;

    if let Some(team_id) = pending.team_id {
        sqlx::query("INSERT INTO team_members (user_id, team_id, role) VALUES ($1, $2, $3)")
            .bind(pending.user_id)
            .bind(team_id)
            .bind(&pending.role)
            .execute(&mut *tx)
            .await?;
    }

    enrollment::insert(&mut tx, crypto, pending.user_id, credential).await?;
    recovery::store(&mut tx, pending.user_id, recovery_codes).await?;

    let session_id =
        session::create_on(&mut tx, pending.user_id, Some(pending.org_id), client).await?;

    tx.commit().await?;
    Ok(Some(session_id))
}

/// Looks up a still-valid invite by its token, with org and team names.
async fn resolve(pool: &PgPool, token: &str) -> sqlx::Result<Option<Resolved>> {
    let row = sqlx::query_as::<
        _,
        (
            Uuid,
            Uuid,
            String,
            Option<Uuid>,
            Option<String>,
            String,
            String,
        ),
    >(
        "SELECT i.id, i.org_id, o.name, i.team_id, t.name, i.role, i.email
           FROM invites i
           JOIN orgs o ON o.id = i.org_id
      LEFT JOIN teams t ON t.id = i.team_id
          WHERE i.token_hash = $1 AND i.accepted_at IS NULL AND i.expires_at > now()",
    )
    .bind(hash(token))
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(id, org_id, org_name, team_id, team_name, role, email)| Resolved {
            id,
            org_id,
            org_name,
            team_id,
            team_name,
            role,
            email,
        },
    ))
}

/// Whether a team exists within the given org.
async fn team_in_org(pool: &PgPool, team_id: Uuid, org_id: Uuid) -> sqlx::Result<bool> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM teams WHERE id = $1 AND org_id = $2)",
    )
    .bind(team_id)
    .bind(org_id)
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

/// Mints a fresh, unguessable invite token.
fn mint_token() -> String {
    let mut bytes = [0u8; TOKEN_BYTES];
    OsRng.fill_bytes(&mut bytes);
    B64.encode(bytes)
}

/// SHA-256-hashes a token to its stored hex form.
fn hash(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Reads the in-flight invite acceptance from the cookie, if present and valid.
fn read_pending(jar: &CookieJar) -> Option<Pending> {
    jar.get(INVITE_COOKIE)
        .and_then(|entry| serde_json::from_str(entry.value()).ok())
}
