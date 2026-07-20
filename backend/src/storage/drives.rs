//! Drive management API: the project-scoped storage control plane.
//!
//! Admins create, link, and manage [`DriveRow`]s; this module is the HTTP surface
//! over that model (see `docs/storage.md`). File transfer (#22) and search (#23) are
//! separate, and both reuse the [`can_access_drive`] helper defined here as the
//! single source of truth for who may reach a Drive.
//!
//! Routes live under `/api/v1/drives` (wired in [`crate::api`]) and speak protobuf.
//! Reads are not capability-gated: [`list`] returns the Drives the caller can access
//! in a project per [`can_access_drive`]. Every management verb (create, update,
//! delete, connection-test, access, default) requires the `manage_storage`
//! capability, resolved through [`can_manage_storage`].
//!
//! # The `manage_storage` gate
//!
//! Managing a Drive is a project-or-org admin action. [`can_manage_storage`] grants
//! it when the caller is a project owner/admin on the Drive's project (org owners and
//! admins bypass to project Owner through [`resolve_project_access`]), or an ordinary
//! org member holding a delegated org-level `manage_storage` grant. This mirrors
//! `docs/storage.md` ("reusing the existing grants/policies/authz machinery") and the
//! project-tier `require_project_admin_or_manage` pattern.
//!
//! # Secrets never leave
//!
//! A Drive's provider credential is sealed with [`seal_secret`] before insert and is
//! never returned: the [`Drive`] message carries only `credential_set`, a boolean.
//! The write-only `credential` request field is sealed and dropped.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::authz::{self, Capability, OrgRole};
use crate::projects::{has_project_access, resolve_project_access, ProjectRole};
use crate::proto::common::v1::{Timestamp, Uuid as ProtoUuid};
use crate::proto::storage::v1::{
    drive_config, CreateDriveRequest, Drive, DriveConfig as ProtoDriveConfig,
    DriveKind as ProtoDriveKind, DriveVisibility as ProtoVisibility, ListDrivesResponse,
    SetDriveAccessRequest, UpdateDriveRequest,
};
use crate::protobuf::Protobuf;
use crate::session::{Principal, SessionUser};
use crate::state::AppState;
use crate::storage::provider;
use crate::users;

use super::{
    open_secret, seal_secret, DriveConfig, DriveKind, DrivePrincipal, DriveRow, DriveVisibility,
};

// --- Access resolution (the shared helpers #22/#23 reuse) -------------------

/// Whether `user_id` may see and use `drive`, the single storage access rule.
///
/// A caller reaches a Drive when they have project access (via
/// [`resolve_project_access`]) **and** the Drive's visibility admits them:
///
/// - **No project access** (`resolve_project_access` is `None`): denied. This also
///   enforces org and cross-project isolation, since a caller never resolves a role
///   on a project outside their reach.
/// - **Project admin or owner** (org owners/admins bypass to project Owner): allowed
///   for every Drive in the project, regardless of visibility.
/// - **Project member** on an `ORG_WIDE` Drive: allowed.
/// - **Project member** on a `RESTRICTED` Drive: allowed only with a `drive_access`
///   grant, held directly as the user or through a team they belong to.
///
/// This is the one function transfer (#22) and search (#23) call to gate a Drive, so
/// the rule lives in exactly one place.
///
/// # Errors
/// Returns an error if any underlying query fails.
pub async fn can_access_drive(
    conn: &mut PgConnection,
    user_id: Uuid,
    drive: &DriveRow,
) -> sqlx::Result<bool> {
    let Some(role) = resolve_project_access(conn, user_id, drive.project_id).await? else {
        return Ok(false);
    };
    // Project admins and owners (org admins bypass to Owner) reach every Drive.
    if role >= ProjectRole::Admin {
        return Ok(true);
    }
    // A plain member reaches an org-wide Drive, or a restricted one they are granted.
    if drive.visibility == DriveVisibility::OrgWide {
        return Ok(true);
    }
    has_drive_grant(conn, drive.id, user_id).await
}

/// Whether a restricted Drive grants `user_id` access directly or through a team.
///
/// Matches a `drive_access` row targeting the user, or one targeting a team the user
/// belongs to. A stray grant to a non-member is inert, since [`can_access_drive`]
/// requires project access first.
async fn has_drive_grant(
    executor: impl sqlx::PgExecutor<'_>,
    drive_id: Uuid,
    user_id: Uuid,
) -> sqlx::Result<bool> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1 FROM drive_access da
              WHERE da.drive_id = $1
                AND (
                    (da.principal = $2 AND da.principal_id = $4)
                    OR (da.principal = $3 AND da.principal_id IN (
                        SELECT team_id FROM team_members WHERE user_id = $4))
                )
         )",
    )
    .bind(drive_id)
    .bind(DrivePrincipal::User.as_db())
    .bind(DrivePrincipal::Team.as_db())
    .bind(user_id)
    .fetch_one(executor)
    .await
}

/// Whether `user_id` may manage Drives in `project_id` (the `manage_storage` gate).
///
/// Granted to a project owner/admin (org owners and admins bypass to project Owner
/// via [`has_project_access`]), or to an ordinary org member holding a delegated
/// org-level `manage_storage` grant. Guests and suspended members never qualify.
///
/// # Errors
/// Returns an error if any underlying query fails.
pub async fn can_manage_storage(
    conn: &mut PgConnection,
    user_id: Uuid,
    org_id: Uuid,
    project_id: Uuid,
) -> sqlx::Result<bool> {
    // Project owners/admins (org admins bypass) may manage their project's Drives.
    if has_project_access(conn, user_id, project_id, ProjectRole::Admin).await? {
        return Ok(true);
    }
    // Otherwise only a non-suspended member with the delegated grant qualifies.
    match authz::membership(&mut *conn, user_id, org_id).await? {
        Some(entry) if !entry.suspended && entry.role == OrgRole::Member => {
            has_manage_storage_grant(&mut *conn, user_id, org_id).await
        }
        _ => Ok(false),
    }
}

/// Whether `user_id` holds a delegated org-level `manage_storage` grant in `org_id`.
async fn has_manage_storage_grant(
    executor: impl sqlx::PgExecutor<'_>,
    user_id: Uuid,
    org_id: Uuid,
) -> sqlx::Result<bool> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM org_admin_grants
                         WHERE org_id = $1 AND user_id = $2 AND capability = $3)",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(Capability::ManageStorage.as_str())
    .fetch_one(executor)
    .await
}

// --- Management API ---------------------------------------------------------

/// Query for [`list`]: the project whose Drives to return.
#[derive(Deserialize)]
pub struct ListDrivesQuery {
    project_id: Uuid,
}

/// `GET /api/v1/drives?project_id=`: the Drives the caller can access in a project.
///
/// Read is not capability-gated: a caller with project access sees every org-wide
/// Drive plus the restricted Drives granted to them, and a project/org admin sees all
/// (see [`can_access_drive`]). A caller with no access to the project gets `403`, so
/// they cannot enumerate a project they are not in.
pub async fn list(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Query(query): Query<ListDrivesQuery>,
) -> Response {
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "drive list user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let mut conn = match state.pool.acquire().await {
        Ok(conn) => conn,
        Err(error) => {
            tracing::error!(error = %error, "drive list connection acquire failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    // No access to the project means no view of its Drives at all.
    match resolve_project_access(&mut conn, user_id, query.project_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return StatusCode::FORBIDDEN.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "drive list access resolution failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    let rows = match sqlx::query_as::<_, DriveRow>(
        "SELECT * FROM drives WHERE project_id = $1 ORDER BY name ASC",
    )
    .bind(query.project_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::error!(error = %error, "drive list query failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let mut drives = Vec::new();
    for row in &rows {
        match can_access_drive(&mut conn, user_id, row).await {
            Ok(true) => drives.push(drive_to_proto(row)),
            Ok(false) => {}
            Err(error) => {
                tracing::error!(error = %error, "drive access resolution failed");
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
        }
    }

    Protobuf(ListDrivesResponse { drives }).into_response()
}

/// `POST /api/v1/drives`: create or link a Drive in a project (`manage_storage`).
///
/// Seals the write-only `credential` before insert and returns the [`Drive`] with
/// `credential_set`, never the secret. Capabilities are derived from the kind.
pub async fn create(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Protobuf(request): Protobuf<CreateDriveRequest>,
) -> Response {
    let Some(project_id) = request.project_id.as_ref().and_then(parse_uuid) else {
        return (StatusCode::BAD_REQUEST, "project_id is required").into_response();
    };
    let org_id = match require_manage_storage(&state, &principal, project_id).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };

    let name = request.name.trim();
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, "name is required").into_response();
    }
    let Some(kind) = parse_kind(request.kind) else {
        return (StatusCode::BAD_REQUEST, "kind is required").into_response();
    };
    let visibility = match parse_visibility(request.visibility) {
        Some(visibility) => visibility,
        // An unset visibility defaults to org-wide, the least surprising default.
        None => DriveVisibility::OrgWide,
    };
    let config = match config_from_proto(kind, request.config.as_ref()) {
        Ok(config) => config,
        Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
    };

    let creator_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "drive create user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    // Seal the provider secret bound to the project; an empty credential means none.
    let secret = if request.credential.is_empty() {
        None
    } else {
        Some(seal_secret(
            &state.crypto,
            project_id,
            request.credential.as_bytes(),
        ))
    };

    let row = sqlx::query_as::<_, DriveRow>(
        "INSERT INTO drives (project_id, org_id, name, kind, config, secret, visibility, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING *",
    )
    .bind(project_id)
    .bind(org_id)
    .bind(name)
    .bind(kind.as_db())
    .bind(config.to_json())
    .bind(secret)
    .bind(visibility.as_db())
    .bind(creator_id)
    .fetch_one(&state.pool)
    .await;

    match row {
        Ok(row) => Protobuf(drive_to_proto(&row)).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "drive creation failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `PATCH /api/v1/drives/{id}`: update a Drive's name, visibility, config, or secret.
///
/// Kind and project are immutable. An empty `name` or unset `visibility`/`config`
/// leaves that field unchanged; an empty `credential` keeps the existing sealed
/// secret, a non-empty one re-seals it.
pub async fn update(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(drive_id): Path<Uuid>,
    Protobuf(request): Protobuf<UpdateDriveRequest>,
) -> Response {
    let row = match load_drive(&state, drive_id).await {
        Ok(row) => row,
        Err(response) => return response,
    };
    if let Err(response) = require_manage_storage(&state, &principal, row.project_id).await {
        return response;
    }

    let name = {
        let trimmed = request.name.trim();
        if trimmed.is_empty() {
            row.name.clone()
        } else {
            trimmed.to_string()
        }
    };
    // An unset (or unknown) visibility keeps the current value.
    let visibility = parse_visibility(request.visibility).unwrap_or(row.visibility);
    let config = match request.config.as_ref() {
        Some(_) => match config_from_proto(row.kind, request.config.as_ref()) {
            Ok(config) => config.to_json(),
            Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
        },
        None => row.config.clone(),
    };
    let secret = if request.credential.is_empty() {
        row.secret.clone()
    } else {
        Some(seal_secret(
            &state.crypto,
            row.project_id,
            request.credential.as_bytes(),
        ))
    };

    let updated = sqlx::query_as::<_, DriveRow>(
        "UPDATE drives
            SET name = $1, visibility = $2, config = $3, secret = $4, updated_at = now()
          WHERE id = $5
      RETURNING *",
    )
    .bind(name)
    .bind(visibility.as_db())
    .bind(config)
    .bind(secret)
    .bind(drive_id)
    .fetch_one(&state.pool)
    .await;

    match updated {
        Ok(row) => Protobuf(drive_to_proto(&row)).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "drive update failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `DELETE /api/v1/drives/{id}`: unlink a Drive (`manage_storage`).
///
/// Removes the metadata and grants; the provider's data is untouched, since Elysium
/// is a gateway, not the source of truth.
pub async fn delete(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(drive_id): Path<Uuid>,
) -> Response {
    let row = match load_drive(&state, drive_id).await {
        Ok(row) => row,
        Err(response) => return response,
    };
    if let Err(response) = require_manage_storage(&state, &principal, row.project_id).await {
        return response;
    }

    match sqlx::query("DELETE FROM drives WHERE id = $1")
        .bind(drive_id)
        .execute(&state.pool)
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "drive deletion failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /api/v1/drives/{id}/test`: probe a Drive's provider (`manage_storage`).
///
/// Returns `200` when the connection reaches the provider, `422` with the failure
/// message otherwise, so an admin can validate a Drive before relying on it.
pub async fn test(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(drive_id): Path<Uuid>,
) -> Response {
    let row = match load_drive(&state, drive_id).await {
        Ok(row) => row,
        Err(response) => return response,
    };
    if let Err(response) = require_manage_storage(&state, &principal, row.project_id).await {
        return response;
    }

    let config = match row.parse_config() {
        Ok(config) => config,
        Err(error) => {
            tracing::error!(error = %error, "stored drive config is malformed");
            return (StatusCode::UNPROCESSABLE_ENTITY, error.to_string()).into_response();
        }
    };
    let secret = match row.secret.as_deref() {
        Some(blob) => match open_secret(&state.crypto, row.project_id, blob) {
            Ok(secret) => Some(secret),
            Err(error) => {
                tracing::error!(error = %error, "drive credential failed to open");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        },
        None => None,
    };

    match provider::test_connection(&config, secret.as_deref()).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(error) => (StatusCode::UNPROCESSABLE_ENTITY, error.to_string()).into_response(),
    }
}

/// `PUT /api/v1/drives/{id}/access`: replace a Drive's access grants (`manage_storage`).
///
/// The request's grants replace any existing `drive_access` rows atomically. Grants
/// only matter for a restricted Drive; setting them on an org-wide Drive is harmless.
pub async fn set_access(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(drive_id): Path<Uuid>,
    Protobuf(request): Protobuf<SetDriveAccessRequest>,
) -> Response {
    let row = match load_drive(&state, drive_id).await {
        Ok(row) => row,
        Err(response) => return response,
    };
    if let Err(response) = require_manage_storage(&state, &principal, row.project_id).await {
        return response;
    }

    // Parse every grant up front so a bad one is a clean 400 before any write.
    let mut grants = Vec::with_capacity(request.grants.len());
    for grant in &request.grants {
        let Some(principal) = parse_principal(grant.principal) else {
            return (StatusCode::BAD_REQUEST, "invalid grant principal").into_response();
        };
        let Some(principal_id) = grant.principal_id.as_ref().and_then(parse_uuid) else {
            return (StatusCode::BAD_REQUEST, "invalid grant principal_id").into_response();
        };
        grants.push((principal, principal_id));
    }

    let outcome = async {
        let mut tx = state.pool.begin().await?;
        sqlx::query("DELETE FROM drive_access WHERE drive_id = $1")
            .bind(drive_id)
            .execute(&mut *tx)
            .await?;
        for (principal, principal_id) in &grants {
            sqlx::query(
                "INSERT INTO drive_access (drive_id, principal, principal_id)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (drive_id, principal, principal_id) DO NOTHING",
            )
            .bind(drive_id)
            .bind(principal.as_db())
            .bind(principal_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }
    .await;

    match outcome {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "drive access update failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /api/v1/drives/{id}/default`: make a Drive its project's default.
///
/// Clears the project's current default and sets this one in a single transaction,
/// respecting the per-project partial unique index. Uploads without an explicit Drive
/// land in the project's default.
pub async fn set_default(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(drive_id): Path<Uuid>,
) -> Response {
    let row = match load_drive(&state, drive_id).await {
        Ok(row) => row,
        Err(response) => return response,
    };
    if let Err(response) = require_manage_storage(&state, &principal, row.project_id).await {
        return response;
    }

    let outcome = async {
        let mut tx = state.pool.begin().await?;
        let row = swap_default(&mut tx, row.project_id, drive_id).await?;
        tx.commit().await?;
        Ok::<_, sqlx::Error>(row)
    }
    .await;
    match outcome {
        Ok(row) => Protobuf(drive_to_proto(&row)).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "drive default swap failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Clears the project's current default Drive and sets `drive_id` as the new one.
///
/// Runs on the caller's connection (a `&mut PgConnection`), so the caller wraps both
/// updates in one transaction and the per-project single-default invariant (the
/// `drives_one_default_per_project` partial unique index) holds throughout. Returns
/// the newly-default Drive row.
///
/// # Errors
/// Returns an error if either update fails.
async fn swap_default(
    conn: &mut PgConnection,
    project_id: Uuid,
    drive_id: Uuid,
) -> sqlx::Result<DriveRow> {
    sqlx::query(
        "UPDATE drives SET is_default = FALSE, updated_at = now()
          WHERE project_id = $1 AND is_default AND id <> $2",
    )
    .bind(project_id)
    .bind(drive_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query_as::<_, DriveRow>(
        "UPDATE drives SET is_default = TRUE, updated_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(drive_id)
    .fetch_one(&mut *conn)
    .await
}

// --- Authorization + loading helpers ----------------------------------------

/// Requires the caller may manage Drives in `project_id`, returning the org id.
///
/// The HTTP guard over [`can_manage_storage`]: it resolves the project's org, the
/// caller's user id, and their authority, mapping each failure to a ready response.
///
/// # Errors
/// Returns a ready `Response`: `404` when the project is unknown, `401` when the
/// session does not resolve to a user, `403` when the caller lacks authority, and
/// `503` on a database error.
async fn require_manage_storage(
    state: &AppState,
    principal: &Principal,
    project_id: Uuid,
) -> Result<Uuid, Response> {
    let org_id = org_for_project(state, project_id).await?;
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return Err(StatusCode::UNAUTHORIZED.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "storage authz user lookup failed");
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };
    let mut conn = match state.pool.acquire().await {
        Ok(conn) => conn,
        Err(error) => {
            tracing::error!(error = %error, "storage authz connection acquire failed");
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };
    match can_manage_storage(&mut conn, user_id, org_id, project_id).await {
        Ok(true) => Ok(org_id),
        Ok(false) => Err(StatusCode::FORBIDDEN.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "storage authz resolution failed");
            Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
    }
}

/// Resolves a project's org, or a ready `404`/`503` response.
async fn org_for_project(state: &AppState, project_id: Uuid) -> Result<Uuid, Response> {
    match sqlx::query_scalar::<_, Uuid>("SELECT org_id FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(org_id)) => Ok(org_id),
        Ok(None) => Err(StatusCode::NOT_FOUND.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "project org lookup failed");
            Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
    }
}

/// Loads a Drive by id, or a ready `404`/`503` response.
async fn load_drive(state: &AppState, drive_id: Uuid) -> Result<DriveRow, Response> {
    match sqlx::query_as::<_, DriveRow>("SELECT * FROM drives WHERE id = $1")
        .bind(drive_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(row)) => Ok(row),
        Ok(None) => Err(StatusCode::NOT_FOUND.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "drive load failed");
            Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
    }
}

// --- Proto conversions ------------------------------------------------------

/// Parses a wire principal to the stored [`DrivePrincipal`]; unset/unknown is `None`.
fn parse_principal(value: i32) -> Option<DrivePrincipal> {
    crate::proto::storage::v1::drive_access::Principal::try_from(value)
        .ok()
        .and_then(DrivePrincipal::from_proto)
}

/// Parses a request's `kind` field to a [`DriveKind`]; `UNSPECIFIED`/unknown is `None`.
fn parse_kind(value: i32) -> Option<DriveKind> {
    ProtoDriveKind::try_from(value)
        .ok()
        .and_then(DriveKind::from_proto)
}

/// Parses a request's `visibility` field to a [`DriveVisibility`]; unset is `None`.
fn parse_visibility(value: i32) -> Option<DriveVisibility> {
    ProtoVisibility::try_from(value)
        .ok()
        .and_then(DriveVisibility::from_proto)
}

/// Parses a proto [`ProtoUuid`] wrapper into a real [`Uuid`], or `None` when invalid.
fn parse_uuid(id: &ProtoUuid) -> Option<Uuid> {
    Uuid::parse_str(id.value.trim()).ok()
}

/// Builds a validated native [`DriveConfig`] from a request's proto config.
///
/// The proto's active oneof group must match `kind`, and its values must pass the
/// same [`DriveConfig::parse`] validation every write goes through.
///
/// # Errors
/// Returns a user-facing message when the config is absent, mismatches the kind, or
/// fails validation.
fn config_from_proto(
    kind: DriveKind,
    config: Option<&ProtoDriveConfig>,
) -> Result<DriveConfig, String> {
    let group = config.and_then(|config| config.kind.as_ref());
    let value = match (kind, group) {
        (DriveKind::Fs, Some(drive_config::Kind::Fs(fs))) => {
            serde_json::json!({ "root": fs.root })
        }
        (DriveKind::S3, Some(drive_config::Kind::S3(s3))) => serde_json::json!({
            "endpoint": s3.endpoint,
            "region": s3.region,
            "bucket": s3.bucket,
            "prefix": s3.prefix,
            "sse": s3.sse,
        }),
        (DriveKind::Sftp, Some(drive_config::Kind::Sftp(sftp))) => serde_json::json!({
            "host": sftp.host,
            "port": sftp.port,
            "user": sftp.user,
            "base_path": sftp.base_path,
        }),
        _ => return Err("config does not match the Drive kind".to_string()),
    };
    DriveConfig::parse(kind, &value).map_err(|error| error.to_string())
}

/// Renders a native [`DriveConfig`] as its proto message for a response.
fn config_to_proto(config: &DriveConfig) -> ProtoDriveConfig {
    let group = match config {
        DriveConfig::Fs(fs) => drive_config::Kind::Fs(drive_config::Fs {
            root: fs.root.clone(),
        }),
        DriveConfig::S3(s3) => drive_config::Kind::S3(drive_config::S3 {
            endpoint: s3.endpoint.clone(),
            region: s3.region.clone(),
            bucket: s3.bucket.clone(),
            prefix: s3.prefix.clone(),
            sse: s3.sse,
        }),
        DriveConfig::Sftp(sftp) => drive_config::Kind::Sftp(drive_config::Sftp {
            host: sftp.host.clone(),
            port: u32::from(sftp.port),
            user: sftp.user.clone(),
            base_path: sftp.base_path.clone(),
        }),
    };
    ProtoDriveConfig { kind: Some(group) }
}

/// Builds the protobuf [`Drive`] for a row, exposing `credential_set`, never the secret.
///
/// Capabilities come from the kind; the config is best-effort (a corrupt stored config
/// yields no config rather than failing the whole response).
fn drive_to_proto(row: &DriveRow) -> Drive {
    Drive {
        id: Some(proto_uuid(row.id)),
        project_id: Some(proto_uuid(row.project_id)),
        org_id: Some(proto_uuid(row.org_id)),
        name: row.name.clone(),
        kind: row.kind.to_proto() as i32,
        visibility: row.visibility.to_proto() as i32,
        is_default: row.is_default,
        credential_set: row.secret.is_some(),
        capabilities: row
            .kind
            .capabilities()
            .iter()
            .map(|capability| *capability as i32)
            .collect(),
        config: row
            .parse_config()
            .ok()
            .map(|config| config_to_proto(&config)),
        created_at: Some(to_timestamp(row.created_at)),
        updated_at: Some(to_timestamp(row.updated_at)),
    }
}

/// Wraps a database `Uuid` in its proto string form.
fn proto_uuid(id: Uuid) -> ProtoUuid {
    ProtoUuid {
        value: id.to_string(),
    }
}

/// Converts a UTC instant into the canonical proto `Timestamp`.
fn to_timestamp(instant: DateTime<Utc>) -> Timestamp {
    Timestamp {
        epoch_seconds: instant.timestamp(),
        nanos: instant.timestamp_subsec_nanos(),
        timezone: "Etc/UTC".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Crypto;
    use crate::proto::storage::v1::{drive_config, DriveConfig as ProtoDriveConfig};
    use crate::storage::FsConfig;
    use prost::Message;

    /// A fixed 32-byte key for deterministic crypto tests, as in `crypto.rs`.
    fn test_crypto() -> Crypto {
        Crypto::new(&[9u8; 32]).expect("32-byte key is valid")
    }

    /// Builds a minimal FS `DriveRow` for pure-conversion tests.
    fn fs_row(secret: Option<Vec<u8>>) -> DriveRow {
        DriveRow {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            name: "Primary".to_string(),
            kind: DriveKind::Fs,
            config: serde_json::json!({ "root": "/srv/data" }),
            secret,
            visibility: DriveVisibility::OrgWide,
            is_default: true,
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    // --- Proto config conversion --------------------------------------------

    #[test]
    fn config_from_proto_round_trips_every_kind() {
        let fs = ProtoDriveConfig {
            kind: Some(drive_config::Kind::Fs(drive_config::Fs {
                root: "/srv/data".to_string(),
            })),
        };
        assert_eq!(
            config_from_proto(DriveKind::Fs, Some(&fs)).expect("valid fs"),
            DriveConfig::Fs(FsConfig {
                root: "/srv/data".to_string(),
            })
        );

        let s3 = ProtoDriveConfig {
            kind: Some(drive_config::Kind::S3(drive_config::S3 {
                endpoint: "https://s3.example.com".to_string(),
                region: "us-east-1".to_string(),
                bucket: "bkt".to_string(),
                prefix: "team/".to_string(),
                sse: true,
            })),
        };
        let parsed = config_from_proto(DriveKind::S3, Some(&s3)).expect("valid s3");
        // Round-tripping back to proto preserves the non-secret settings.
        assert_eq!(config_to_proto(&parsed), s3);

        let sftp = ProtoDriveConfig {
            kind: Some(drive_config::Kind::Sftp(drive_config::Sftp {
                host: "files.example.com".to_string(),
                port: 2222,
                user: "elysium".to_string(),
                base_path: "/data".to_string(),
            })),
        };
        let parsed = config_from_proto(DriveKind::Sftp, Some(&sftp)).expect("valid sftp");
        assert_eq!(config_to_proto(&parsed), sftp);
    }

    #[test]
    fn config_from_proto_rejects_a_kind_mismatch_and_absence() {
        // An S3 group under an FS kind is a mismatch, not a silent coercion.
        let s3 = ProtoDriveConfig {
            kind: Some(drive_config::Kind::S3(drive_config::S3 {
                bucket: "bkt".to_string(),
                ..Default::default()
            })),
        };
        assert!(config_from_proto(DriveKind::Fs, Some(&s3)).is_err());
        // No config at all is rejected too.
        assert!(config_from_proto(DriveKind::Fs, None).is_err());
    }

    #[test]
    fn config_from_proto_enforces_validation() {
        // A relative FS root fails the same validation every write goes through.
        let fs = ProtoDriveConfig {
            kind: Some(drive_config::Kind::Fs(drive_config::Fs {
                root: "relative/path".to_string(),
            })),
        };
        assert!(config_from_proto(DriveKind::Fs, Some(&fs)).is_err());
    }

    // --- Response shape + secret safety -------------------------------------

    #[test]
    fn drive_to_proto_reports_capabilities_and_credential_flag() {
        let row = fs_row(None);
        let proto = drive_to_proto(&row);
        assert_eq!(proto.kind, DriveKind::Fs.to_proto() as i32);
        assert!(!proto.credential_set, "no secret means credential unset");
        // FS advertises all three capabilities (docs/storage.md).
        assert_eq!(proto.capabilities.len(), 3);
        assert!(proto.config.is_some(), "a valid stored config is surfaced");
    }

    #[test]
    fn a_sealed_secret_never_appears_in_the_encoded_drive() {
        let crypto = test_crypto();
        let project_id = Uuid::new_v4();
        let plaintext = b"super-secret-access-key";
        let sealed = seal_secret(&crypto, project_id, plaintext);

        let mut row = fs_row(Some(sealed.clone()));
        row.project_id = project_id;

        let proto = drive_to_proto(&row);
        assert!(proto.credential_set, "a sealed secret sets the flag");

        // The wire-encoded Drive carries the boolean, never the secret bytes, and
        // never the sealed blob either.
        let mut buffer = Vec::new();
        proto.encode(&mut buffer).expect("encode Drive");
        assert!(
            !contains_subslice(&buffer, plaintext),
            "the plaintext secret must not be in the response",
        );
        assert!(
            !contains_subslice(&buffer, &sealed),
            "the sealed secret blob must not be in the response",
        );
    }

    /// Whether `haystack` contains `needle` as a contiguous subslice.
    fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    // --- DB-backed access + management matrix -------------------------------
    //
    // Ignored by default (needs a reachable postgres), like `tests/migrations.rs`
    // and the projects resolver tests. Run with `cargo test -- --ignored` and a
    // DATABASE_URL. Everything happens inside a rolled-back transaction.

    use crate::projects::{add_member, ProjectRole};

    async fn connect_test_pool() -> sqlx::PgPool {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for this test");
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("could not connect to postgres");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations must apply cleanly");
        pool
    }

    async fn insert_user(conn: &mut PgConnection, tag: &str) -> Uuid {
        let unique = Uuid::new_v4();
        sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO users (subject, email) VALUES ($1, $2) RETURNING id",
        )
        .bind(format!("{tag}-{unique}"))
        .bind(format!("{tag}-{unique}@example.test"))
        .fetch_one(conn)
        .await
        .expect("insert user")
    }

    async fn insert_org(conn: &mut PgConnection, tag: &str) -> Uuid {
        let unique = Uuid::new_v4();
        sqlx::query_scalar::<_, Uuid>("INSERT INTO orgs (name, slug) VALUES ($1, $2) RETURNING id")
            .bind(format!("{tag} Org"))
            .bind(format!("{tag}-{unique}"))
            .fetch_one(conn)
            .await
            .expect("insert org")
    }

    async fn add_org_member(conn: &mut PgConnection, org: Uuid, user: Uuid, role: &str) {
        sqlx::query("INSERT INTO org_members (user_id, org_id, role) VALUES ($1, $2, $3)")
            .bind(user)
            .bind(org)
            .bind(role)
            .execute(conn)
            .await
            .expect("add org member");
    }

    async fn insert_project(conn: &mut PgConnection, org: Uuid, slug: &str) -> Uuid {
        sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO projects (org_id, name, slug) VALUES ($1, $2, $2) RETURNING id",
        )
        .bind(org)
        .bind(slug)
        .fetch_one(conn)
        .await
        .expect("insert project")
    }

    async fn insert_team(conn: &mut PgConnection, org: Uuid, project: Uuid, slug: &str) -> Uuid {
        sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO teams (org_id, name, slug, project_id) VALUES ($1, $2, $2, $3)
             RETURNING id",
        )
        .bind(org)
        .bind(slug)
        .bind(project)
        .fetch_one(conn)
        .await
        .expect("insert team")
    }

    async fn add_team_member(conn: &mut PgConnection, team: Uuid, user: Uuid, role: &str) {
        sqlx::query("INSERT INTO team_members (user_id, team_id, role) VALUES ($1, $2, $3)")
            .bind(user)
            .bind(team)
            .bind(role)
            .execute(conn)
            .await
            .expect("add team member");
    }

    async fn insert_drive(
        conn: &mut PgConnection,
        project: Uuid,
        org: Uuid,
        name: &str,
        visibility: DriveVisibility,
        is_default: bool,
    ) -> DriveRow {
        sqlx::query_as::<_, DriveRow>(
            "INSERT INTO drives (project_id, org_id, name, kind, config, visibility, is_default)
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING *",
        )
        .bind(project)
        .bind(org)
        .bind(name)
        .bind(DriveKind::Fs.as_db())
        .bind(serde_json::json!({ "root": "/srv/data" }))
        .bind(visibility.as_db())
        .bind(is_default)
        .fetch_one(conn)
        .await
        .expect("insert drive")
    }

    async fn grant_drive(
        conn: &mut PgConnection,
        drive: Uuid,
        principal: DrivePrincipal,
        principal_id: Uuid,
    ) {
        sqlx::query(
            "INSERT INTO drive_access (drive_id, principal, principal_id) VALUES ($1,$2,$3)",
        )
        .bind(drive)
        .bind(principal.as_db())
        .bind(principal_id)
        .execute(conn)
        .await
        .expect("grant drive access");
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn can_access_drive_covers_every_visibility_and_tier() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");

        let org = insert_org(&mut tx, "acme").await;
        let project_a = insert_project(&mut tx, org, "alpha").await;
        let project_b = insert_project(&mut tx, org, "beta").await;

        let org_wide = insert_drive(
            &mut tx,
            project_a,
            org,
            "shared",
            DriveVisibility::OrgWide,
            true,
        )
        .await;
        let restricted = insert_drive(
            &mut tx,
            project_a,
            org,
            "locked",
            DriveVisibility::Restricted,
            false,
        )
        .await;
        let restricted_b = insert_drive(
            &mut tx,
            project_b,
            org,
            "locked-b",
            DriveVisibility::Restricted,
            false,
        )
        .await;

        // Org admin bypasses to every Drive, restricted included.
        let org_admin = insert_user(&mut tx, "admin").await;
        add_org_member(&mut tx, org, org_admin, "admin").await;
        assert!(can_access_drive(&mut tx, org_admin, &restricted)
            .await
            .unwrap());

        // A full org member reaches an org-wide Drive, but not a restricted one
        // without a grant.
        let member = insert_user(&mut tx, "member").await;
        add_org_member(&mut tx, org, member, "member").await;
        assert!(can_access_drive(&mut tx, member, &org_wide).await.unwrap());
        assert!(!can_access_drive(&mut tx, member, &restricted)
            .await
            .unwrap());

        // A direct user grant opens the restricted Drive for that member.
        grant_drive(&mut tx, restricted.id, DrivePrincipal::User, member).await;
        assert!(can_access_drive(&mut tx, member, &restricted)
            .await
            .unwrap());

        // A team grant opens it for a guest who reaches the project through that team.
        let team = insert_team(&mut tx, org, project_a, "readers").await;
        let team_member = insert_user(&mut tx, "team").await;
        add_org_member(&mut tx, org, team_member, "guest").await;
        add_team_member(&mut tx, team, team_member, "member").await;
        let team_only = insert_drive(
            &mut tx,
            project_a,
            org,
            "team-locked",
            DriveVisibility::Restricted,
            false,
        )
        .await;
        assert!(
            !can_access_drive(&mut tx, team_member, &team_only)
                .await
                .unwrap(),
            "no grant yet, so the restricted Drive is closed",
        );
        grant_drive(&mut tx, team_only.id, DrivePrincipal::Team, team).await;
        assert!(
            can_access_drive(&mut tx, team_member, &team_only)
                .await
                .unwrap(),
            "the team grant opens the restricted Drive",
        );

        // A guest with no project access sees nothing, even an org-wide Drive.
        let outsider = insert_user(&mut tx, "outsider").await;
        add_org_member(&mut tx, org, outsider, "guest").await;
        assert!(!can_access_drive(&mut tx, outsider, &org_wide)
            .await
            .unwrap());

        // Cross-project isolation: a member scoped to project A only cannot reach a
        // Drive in project B.
        let scoped = insert_user(&mut tx, "scoped").await;
        add_org_member(&mut tx, org, scoped, "guest").await;
        add_member(&mut tx, project_a, scoped, ProjectRole::Member)
            .await
            .unwrap();
        assert!(can_access_drive(&mut tx, scoped, &org_wide).await.unwrap());
        assert!(!can_access_drive(&mut tx, scoped, &restricted_b)
            .await
            .unwrap());

        tx.rollback().await.expect("rollback transaction");
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn can_manage_storage_gates_on_admin_or_delegation() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");

        let org = insert_org(&mut tx, "gate").await;
        let project = insert_project(&mut tx, org, "proj").await;

        // A project admin (a scoped guest raised to Admin) may manage storage.
        let project_admin = insert_user(&mut tx, "padmin").await;
        add_org_member(&mut tx, org, project_admin, "guest").await;
        add_member(&mut tx, project, project_admin, ProjectRole::Admin)
            .await
            .unwrap();
        assert!(can_manage_storage(&mut tx, project_admin, org, project)
            .await
            .unwrap());

        // A plain org member (project Member via cascade) may not.
        let plain = insert_user(&mut tx, "plain").await;
        add_org_member(&mut tx, org, plain, "member").await;
        assert!(!can_manage_storage(&mut tx, plain, org, project)
            .await
            .unwrap());

        // The same member with a delegated org-level manage_storage grant may.
        sqlx::query(
            "INSERT INTO org_admin_grants (org_id, user_id, capability)
             VALUES ($1, $2, 'manage_storage')",
        )
        .bind(org)
        .bind(plain)
        .execute(&mut *tx)
        .await
        .expect("delegate manage_storage");
        assert!(can_manage_storage(&mut tx, plain, org, project)
            .await
            .unwrap());

        // An org admin always may; a guest with no project role never may.
        let org_admin = insert_user(&mut tx, "oadmin").await;
        add_org_member(&mut tx, org, org_admin, "admin").await;
        assert!(can_manage_storage(&mut tx, org_admin, org, project)
            .await
            .unwrap());

        let guest = insert_user(&mut tx, "guest").await;
        add_org_member(&mut tx, org, guest, "guest").await;
        assert!(!can_manage_storage(&mut tx, guest, org, project)
            .await
            .unwrap());

        tx.rollback().await.expect("rollback transaction");
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn swap_default_moves_the_default_within_the_project() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");

        let org = insert_org(&mut tx, "def").await;
        let project = insert_project(&mut tx, org, "proj").await;
        let first = insert_drive(
            &mut tx,
            project,
            org,
            "first",
            DriveVisibility::OrgWide,
            true,
        )
        .await;
        let second = insert_drive(
            &mut tx,
            project,
            org,
            "second",
            DriveVisibility::OrgWide,
            false,
        )
        .await;

        // Promoting the second Drive clears the first, leaving exactly one default.
        let promoted = swap_default(&mut tx, project, second.id)
            .await
            .expect("swap default");
        assert!(promoted.is_default, "the promoted Drive is now the default");
        assert_eq!(promoted.id, second.id);

        let defaults = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM drives WHERE project_id = $1 AND is_default",
        )
        .bind(project)
        .fetch_all(&mut *tx)
        .await
        .expect("read defaults");
        assert_eq!(defaults, vec![second.id], "only the new Drive is default");
        assert_ne!(first.id, second.id);

        tx.rollback().await.expect("rollback transaction");
    }
}
