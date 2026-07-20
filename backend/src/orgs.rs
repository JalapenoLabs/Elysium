//! Organizations: creating orgs beyond the first-run root.
//!
//! First-run claim mints the root org; this lets an authenticated profile create
//! further orgs on the same deployment (the NavarroCity / JalapenoLabs / MooresLabAI
//! model). The creator becomes the org's owner and the org gets a default auth
//! policy. Any authenticated user may create an org for now; finer-grained control
//! is a later authorization step. See `docs/identity.md`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::audit;
use crate::authz;
use crate::proto::common::v1::Uuid as ProtoUuid;
use crate::proto::realtime::v1::event_envelope::Payload;
use crate::proto::realtime::v1::OrgUpdated;
use crate::session::{self, SessionUser};
use crate::state::AppState;
use crate::{events, projects, users};

/// Body of the create-org request.
#[derive(Deserialize)]
pub struct CreateOrg {
    name: String,
}

/// `POST /api/v1/orgs`: create a new org, owned by the caller, and switch to it.
pub async fn create(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    jar: CookieJar,
    Json(body): Json<CreateOrg>,
) -> Response {
    let name = body.name.trim();
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, "name is required").into_response();
    }

    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let slug = slugify(name);
    let org_id = match provision(&state.pool, name, &slug, user_id).await {
        Ok(org_id) => org_id,
        Err(error) => {
            tracing::error!(error = %error, "org creation failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Land the caller in the org they just created.
    if let Some(session_id) = session::session_id(&jar) {
        if let Err(error) = session::switch_org(&state.pool, session_id, org_id).await {
            tracing::error!(error = %error, "failed to switch to new org");
        }
    }

    Json(json!({ "id": org_id, "name": name, "slug": slug })).into_response()
}

/// Body of the org-update request. Fields are optional; unset ones are unchanged.
#[derive(Deserialize)]
pub struct UpdateOrg {
    name: Option<String>,
    logo_url: Option<String>,
    accent_color: Option<String>,
    wordmark: Option<String>,
}

/// `PATCH /api/v1/orgs/:id`: update an org's name, logo, and branding (admin only).
///
/// On success, fans out an `org.updated` realtime event to every connected member,
/// so their views reflect the change live (see `docs/realtime.md`).
pub async fn update(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(org_id): Path<Uuid>,
    Json(body): Json<UpdateOrg>,
) -> Response {
    // Only an owner or admin of the org may edit it.
    if let Err(response) =
        authz::require_org_role(&state, &principal, org_id, authz::OrgRole::Admin).await
    {
        return response;
    }

    // Validate the accent color when present: empty (reset) or a `#RRGGBB` hex.
    if let Some(color) = body.accent_color.as_deref().map(str::trim) {
        if !color.is_empty() && !is_hex_color(color) {
            return (StatusCode::BAD_REQUEST, "accent_color must be #RRGGBB").into_response();
        }
    }

    // COALESCE keeps a field unchanged when its bind is NULL (the client omitted it).
    let updated = sqlx::query_as::<_, (String, String, String, String)>(
        "UPDATE orgs
            SET name = COALESCE($1, name),
                logo_url = COALESCE($2, logo_url),
                accent_color = COALESCE($3, accent_color),
                wordmark = COALESCE($4, wordmark)
          WHERE id = $5
      RETURNING name, logo_url, accent_color, wordmark",
    )
    .bind(body.name.as_deref().map(str::trim))
    .bind(body.logo_url.as_deref().map(str::trim))
    .bind(body.accent_color.as_deref().map(str::trim))
    .bind(body.wordmark.as_deref().map(str::trim))
    .bind(org_id)
    .fetch_optional(&state.pool)
    .await;

    let (name, logo_url, accent_color, wordmark) = match updated {
        Ok(Some(row)) => row,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "org update failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    events::publish_org(
        &state,
        org_id,
        Payload::OrgUpdated(OrgUpdated {
            id: Some(ProtoUuid {
                value: org_id.to_string(),
            }),
            name: name.clone(),
            logo_url: logo_url.clone(),
            accent_color: accent_color.clone(),
            wordmark: wordmark.clone(),
        }),
    )
    .await;

    Json(json!({
        "id": org_id,
        "name": name,
        "logo_url": logo_url,
        "accent_color": accent_color,
        "wordmark": wordmark,
    }))
    .into_response()
}

/// Whether `value` is a `#RRGGBB` hex color.
fn is_hex_color(value: &str) -> bool {
    let Some(digits) = value.strip_prefix('#') else {
        return false;
    };
    digits.len() == 6 && digits.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Body of the ownership-transfer request.
#[derive(Deserialize)]
pub struct TransferOwnership {
    user_id: Uuid,
}

/// `POST /api/v1/orgs/:id/transfer-ownership`: hand ownership to another member.
///
/// Owner-only. Promotes the target to owner and demotes the caller to admin in one
/// transaction, so the org always has exactly the intended owner and the previous
/// owner is never left stuck. The target must be an active (non-suspended) member.
pub async fn transfer_ownership(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(org_id): Path<Uuid>,
    Json(body): Json<TransferOwnership>,
) -> Response {
    let actor_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    // Only an owner may transfer ownership.
    match authz::role_in_org(&state.pool, actor_id, org_id).await {
        Ok(Some(authz::OrgRole::Owner)) => {}
        Ok(_) => return StatusCode::FORBIDDEN.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "owner check failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    let target_id = body.user_id;
    if target_id == actor_id {
        return (StatusCode::BAD_REQUEST, "already the owner").into_response();
    }
    // The target must be an active member of this org.
    match authz::membership(&state.pool, target_id, org_id).await {
        Ok(Some(entry)) if !entry.suspended => {}
        Ok(_) => return (StatusCode::BAD_REQUEST, "not an active member").into_response(),
        Err(error) => {
            tracing::error!(error = %error, "target lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    let outcome = async {
        let mut tx = state.pool.begin().await?;
        sqlx::query("UPDATE org_members SET role = 'owner' WHERE user_id = $1 AND org_id = $2")
            .bind(target_id)
            .bind(org_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE org_members SET role = 'admin' WHERE user_id = $1 AND org_id = $2")
            .bind(actor_id)
            .bind(org_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    }
    .await;

    if let Err(error) = outcome {
        tracing::error!(error = %error, "ownership transfer failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    audit::record(
        &state.pool,
        org_id,
        actor_id,
        "org.ownership_transferred",
        Some(target_id),
        json!({}),
    )
    .await;
    // Both parties' roles changed; nudge their clients to re-read context.
    events::announce_session_change(&state, &principal.sub, "ownership_transferred");
    if let Ok(Some(subject)) = users::subject_for_id(&state.pool, target_id).await {
        events::announce_session_change(&state, &subject, "ownership_transferred");
    }
    StatusCode::NO_CONTENT.into_response()
}

/// Creates the org, its default auth policy, and the creator's owner membership.
///
/// # Errors
/// Returns an error if any insert fails.
async fn provision(
    pool: &sqlx::PgPool,
    name: &str,
    slug: &str,
    user_id: Uuid,
) -> sqlx::Result<Uuid> {
    let mut tx = pool.begin().await?;
    let org_id =
        sqlx::query_scalar::<_, Uuid>("INSERT INTO orgs (name, slug) VALUES ($1, $2) RETURNING id")
            .bind(name)
            .bind(slug)
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query("INSERT INTO org_auth_policies (org_id) VALUES ($1)")
        .bind(org_id)
        .execute(&mut *tx)
        .await?;
    users::add_membership(&mut tx, user_id, org_id, "owner").await?;

    // Every org has at least one project: seed the default and make the creator its
    // owner. The M1 default Drive will attach here (see `crate::projects`).
    projects::create_default(&mut tx, org_id, user_id).await?;

    tx.commit().await?;
    Ok(org_id)
}

/// Builds a URL-safe, unique slug from a display name.
///
/// Non-alphanumeric runs collapse to single hyphens; a short random suffix keeps
/// it unique without a retry loop (the `orgs.slug` and per-org `teams.slug` columns
/// are unique). Shared with team creation ([`crate::teams`]).
pub(crate) fn slugify(name: &str) -> String {
    let mut base = String::new();
    let mut last_dash = true;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            base.extend(ch.to_lowercase());
            last_dash = false;
        } else if !last_dash {
            base.push('-');
            last_dash = true;
        }
    }
    let base = base.trim_matches('-');

    let mut suffix = [0u8; 3];
    OsRng.fill_bytes(&mut suffix);
    let suffix: String = suffix.iter().map(|byte| format!("{byte:02x}")).collect();

    if base.is_empty() {
        format!("org-{suffix}")
    } else {
        format!("{base}-{suffix}")
    }
}
