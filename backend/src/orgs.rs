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

use crate::authz;
use crate::proto::common::v1::Uuid as ProtoUuid;
use crate::proto::realtime::v1::event_envelope::Payload;
use crate::proto::realtime::v1::OrgUpdated;
use crate::session::{self, SessionUser};
use crate::state::AppState;
use crate::{events, users};

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
}

/// `PATCH /api/v1/orgs/:id`: update an org's name/logo (owner or admin only).
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

    // COALESCE keeps a field unchanged when its bind is NULL (the client omitted it).
    let updated = sqlx::query_as::<_, (String, String)>(
        "UPDATE orgs SET name = COALESCE($1, name), logo_url = COALESCE($2, logo_url)
          WHERE id = $3 RETURNING name, logo_url",
    )
    .bind(body.name.as_deref().map(str::trim))
    .bind(body.logo_url.as_deref().map(str::trim))
    .bind(org_id)
    .fetch_optional(&state.pool)
    .await;

    let (name, logo_url) = match updated {
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
        }),
    )
    .await;

    Json(json!({ "id": org_id, "name": name, "logo_url": logo_url })).into_response()
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
    tx.commit().await?;
    Ok(org_id)
}

/// Builds a URL-safe, deployment-unique slug from a display name.
///
/// Non-alphanumeric runs collapse to single hyphens; a short random suffix keeps
/// it unique without a retry loop (the `orgs.slug` column is unique).
fn slugify(name: &str) -> String {
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
