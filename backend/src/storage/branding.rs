//! Avatar and org-logo blobs stored on the project's default Drive.
//!
//! User avatars and org logos are Drive-backed, not opaque external URLs: an upload
//! streams the image into the reserved `.elysium/avatars/` or `.elysium/logos/` area
//! of the owning org's **default Drive** (see `docs/storage.md`, "Default Drive and
//! system files"), and the `users.avatar_url` / `orgs.logo_url` column holds a stable
//! **internal reference** to that blob. A read endpoint streams the blob back through
//! the storage layer with the right content type.
//!
//! # The reference scheme
//!
//! The column stores one of two things, told apart by prefix:
//!
//! - An **internal reference** `elysium://<drive_id>/<key>` written by an upload here.
//!   It self-locates: the `drive_id` is embedded, so the blob resolves even after the
//!   project changes its default Drive (a later upload lands on the new default, but an
//!   existing avatar keeps pointing at the Drive it was written to). This is why
//!   changing the default Drive never orphans an already-set avatar.
//! - An **external URL** (`http://` / `https://`) set by the plain profile/branding
//!   update path. Uploads never touch it unless the user replaces the image.
//!
//! [`is_internal_reference`] and [`parse_reference`] are the single place this
//! distinction lives. Response producers call [`present_avatar_url`] /
//! [`present_logo_url`] to turn an internal reference into the served path
//! (`/api/v1/users/{id}/avatar`, `/api/v1/orgs/{id}/logo`) a client can render, while
//! passing an external URL through untouched.
//!
//! # Constraints
//!
//! An upload is gated by an image content-type allow-list ([`extension_for`], no SVG
//! to avoid inline-script surface) and a [`MAX_IMAGE_BYTES`] size cap, and it writes
//! under the reserved `.elysium/` prefix the file browser hides. Because images are
//! small and bounded, the body is validated whole before a single [`Operator::write`],
//! so a rejected upload never leaves a partial blob.

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE, LOCATION};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use sqlx::PgPool;
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::authz::{self, OrgRole};
use crate::session::SessionUser;
use crate::state::AppState;
use crate::users;

use super::provider::{self, DriveOperator};
use super::{open_secret, DriveRow};

/// The scheme marking a Drive-backed image reference in `avatar_url` / `logo_url`.
///
/// The full reference is `elysium://<drive_id>/<key>`. A value starting with anything
/// else (`http://`, `https://`, or empty) is an external URL or unset.
const INTERNAL_SCHEME: &str = "elysium://";

/// Reserved key prefix for avatar blobs, under the Drive's hidden `.elysium/` area.
const AVATAR_PREFIX: &str = ".elysium/avatars";

/// Reserved key prefix for org-logo blobs, under the Drive's hidden `.elysium/` area.
const LOGO_PREFIX: &str = ".elysium/logos";

/// Per-image upload ceiling, in bytes (8 MiB).
///
/// Avatars and logos are small; a generous 8 MiB bounds abuse while comfortably
/// admitting a high-resolution PNG. Independent of the per-org file-transfer cap
/// (`org_storage_settings.max_upload_bytes`), which governs the general file gateway,
/// not these fixed-purpose system blobs.
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

// --- Reference scheme (the internal/external discriminator) ------------------

/// Whether `value` is a Drive-backed internal reference rather than an external URL.
///
/// The one predicate that tells an `elysium://<drive_id>/<key>` reference from an
/// `http(s)://` URL or an empty column, so the two never get confused.
pub fn is_internal_reference(value: &str) -> bool {
    value.starts_with(INTERNAL_SCHEME)
}

/// Builds the internal reference stored in the column for a blob on a Drive.
fn internal_reference(drive_id: Uuid, key: &str) -> String {
    format!("{INTERNAL_SCHEME}{drive_id}/{key}")
}

/// Parses an internal reference into its `(drive_id, key)`, or `None` if not internal.
///
/// Returns `None` for an external URL, an empty value, or a malformed reference (a
/// non-UUID authority, or no key after the Drive id).
pub fn parse_reference(value: &str) -> Option<(Uuid, String)> {
    let rest = value.strip_prefix(INTERNAL_SCHEME)?;
    let (drive, key) = rest.split_once('/')?;
    if key.is_empty() {
        return None;
    }
    let drive_id = Uuid::parse_str(drive).ok()?;
    Some((drive_id, key.to_string()))
}

/// The served path a client fetches to render a user's avatar.
fn avatar_path(user_id: Uuid) -> String {
    format!("/api/v1/users/{user_id}/avatar")
}

/// The served path a client fetches to render an org's logo.
fn logo_path(org_id: Uuid) -> String {
    format!("/api/v1/orgs/{org_id}/logo")
}

/// Presents a stored `avatar_url` as a client-usable URL.
///
/// An internal reference becomes the served [`avatar_path`]; an external URL (or an
/// empty value) passes through untouched. Response producers use this so a client
/// never sees the raw `elysium://` scheme.
pub fn present_avatar_url(user_id: Uuid, stored: &str) -> String {
    if is_internal_reference(stored) {
        avatar_path(user_id)
    } else {
        stored.to_string()
    }
}

/// Presents a stored `logo_url` as a client-usable URL, mirroring [`present_avatar_url`].
pub fn present_logo_url(org_id: Uuid, stored: &str) -> String {
    if is_internal_reference(stored) {
        logo_path(org_id)
    } else {
        stored.to_string()
    }
}

// --- Content-type allow-list -------------------------------------------------

/// The canonical extension for an allowed image content type, or `None` if rejected.
///
/// SVG is intentionally absent: served inline it can carry script, so it stays out of
/// the avatar/logo surface. The content type is matched on its bare MIME, ignoring any
/// `; charset=` parameter.
fn extension_for(content_type: &str) -> Option<&'static str> {
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

/// The content type to serve a stored blob with, keyed off its stored extension.
///
/// Falls back to `application/octet-stream` for an unknown extension, so a served blob
/// always carries a definite, non-executable type (paired with the API's global
/// `X-Content-Type-Options: nosniff`).
fn content_type_for_key(key: &str) -> &'static str {
    match key.rsplit('.').next() {
        Some("png") => "image/png",
        Some("jpg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    }
}

// --- Avatar endpoints --------------------------------------------------------

/// `PUT /api/v1/users/me/avatar`: store the caller's avatar on the default Drive.
///
/// Validates the image content type and size, streams it into
/// `.elysium/avatars/{user_id}.{ext}` on the active org's default Drive, records the
/// internal reference in `users.avatar_url`, and returns the served path as JSON. An
/// external `avatar_url` set previously is replaced only by this successful upload.
pub async fn upload_avatar(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    request: Request,
) -> Response {
    let Some(org_id) = principal.active_org_id else {
        return (StatusCode::BAD_REQUEST, "no active organization").into_response();
    };
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "avatar upload user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let key_stem = format!("{AVATAR_PREFIX}/{user_id}");
    store_image(
        &state,
        org_id,
        &key_stem,
        request,
        "avatar_url",
        UpdateTarget::User(user_id),
    )
    .await
    .map_or_else(
        |response| response,
        |path| Json(json!({ "url": path })).into_response(),
    )
}

/// `GET /api/v1/users/{user_id}/avatar`: stream a user's avatar image.
///
/// Reads `users.avatar_url`: an internal reference streams the blob from its Drive with
/// the right content type; an external URL redirects (`302`); an empty value is `404`.
/// Any authenticated user may fetch any avatar, matching how avatars are already shared
/// to teammates.
pub async fn serve_avatar(
    State(state): State<AppState>,
    SessionUser(_): SessionUser,
    Path(user_id): Path<Uuid>,
) -> Response {
    let stored = sqlx::query_scalar::<_, String>("SELECT avatar_url FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await;
    serve_stored(&state, stored, "avatar").await
}

// --- Logo endpoints ----------------------------------------------------------

/// `PUT /api/v1/orgs/{id}/logo`: store an org's logo on its default Drive (admin only).
///
/// Mirrors [`upload_avatar`] for an org: an owner/admin uploads an image, it lands in
/// `.elysium/logos/{org_id}.{ext}` on the org's default Drive, and `orgs.logo_url`
/// records the internal reference. Returns the served path as JSON.
pub async fn upload_logo(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(org_id): Path<Uuid>,
    request: Request,
) -> Response {
    if let Err(response) = authz::require_org_role(&state, &principal, org_id, OrgRole::Admin).await
    {
        return response;
    }

    let key_stem = format!("{LOGO_PREFIX}/{org_id}");
    store_image(
        &state,
        org_id,
        &key_stem,
        request,
        "logo_url",
        UpdateTarget::Org(org_id),
    )
    .await
    .map_or_else(
        |response| response,
        |path| Json(json!({ "url": path })).into_response(),
    )
}

/// `GET /api/v1/orgs/{id}/logo`: stream an org's logo image, mirroring [`serve_avatar`].
pub async fn serve_logo(
    State(state): State<AppState>,
    SessionUser(_): SessionUser,
    Path(org_id): Path<Uuid>,
) -> Response {
    let stored = sqlx::query_scalar::<_, String>("SELECT logo_url FROM orgs WHERE id = $1")
        .bind(org_id)
        .fetch_optional(&state.pool)
        .await;
    serve_stored(&state, stored, "logo").await
}

// --- Shared upload/serve plumbing --------------------------------------------

/// Which row's URL column a successful upload updates.
enum UpdateTarget {
    /// Update `users.avatar_url` for this user id.
    User(Uuid),
    /// Update `orgs.logo_url` for this org id.
    Org(Uuid),
}

/// Validates, stores, and records an uploaded image, returning its served path.
///
/// Resolves the org's default Drive, checks the content type and size, writes the blob
/// to `{key_stem}.{ext}`, deletes any prior internal blob at a different key (so a
/// format change never orphans the old one), and sets `column` to the internal
/// reference. `Ok` carries the served path; `Err` is a ready HTTP response.
async fn store_image(
    state: &AppState,
    org_id: Uuid,
    key_stem: &str,
    request: Request,
    column: &str,
    target: UpdateTarget,
) -> Result<String, Response> {
    let content_type = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let Some(extension) = extension_for(content_type) else {
        return Err((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "avatar/logo must be a PNG, JPEG, GIF, or WebP image",
        )
            .into_response());
    };

    // Reject an oversize upload up front when the client declares its length.
    if let Some(declared) = declared_length(&request) {
        if declared > MAX_IMAGE_BYTES {
            return Err(too_large());
        }
    }

    let drive = default_drive_for_org(&state.pool, org_id).await?;
    let operator = open_operator(state, &drive).map_err(|error| {
        tracing::error!(error = %error, "failed to open default Drive for image upload");
        StatusCode::BAD_GATEWAY.into_response()
    })?;

    // Learn the current reference before overwriting it, so a superseded blob (e.g. a
    // prior extension on the same Drive) can be cleaned up after the new one commits.
    let previous = read_reference(&state.pool, column, &target)
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "failed to read current image reference");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        })?;

    let bytes = read_capped(request.into_body(), MAX_IMAGE_BYTES).await?;

    let key = format!("{key_stem}.{extension}");
    operator.write(&key, bytes).await.map_err(|error| {
        tracing::error!(error = %error, "failed to write image blob to Drive");
        StatusCode::BAD_GATEWAY.into_response()
    })?;

    let reference = internal_reference(drive.id, &key);
    if let Err(error) = record_reference(&state.pool, column, &target, &reference).await {
        tracing::error!(error = %error, "failed to record image reference");
        return Err(StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }

    // Best-effort: remove a superseded blob on the same Drive at a different key (a
    // format change), so it never orphans. A blob on a since-changed default Drive is
    // left in place (the documented "changing the default Drive doesn't migrate
    // existing `.elysium/` files" caveat), since the new reference already points here.
    if let Some((old_drive, old_key)) = previous.as_deref().and_then(parse_reference) {
        if old_drive == drive.id && old_key != key {
            if let Err(error) = operator.delete(&old_key).await {
                tracing::debug!(error = %error, "could not remove superseded image blob");
            }
        }
    }

    Ok(match target {
        UpdateTarget::User(user_id) => avatar_path(user_id),
        UpdateTarget::Org(org_id) => logo_path(org_id),
    })
}

/// Streams a stored image reference back to the client, or redirects an external URL.
async fn serve_stored(
    state: &AppState,
    stored: sqlx::Result<Option<String>>,
    what: &str,
) -> Response {
    let stored = match stored {
        Ok(Some(value)) => value,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "{what} lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    if stored.is_empty() {
        return StatusCode::NOT_FOUND.into_response();
    }

    // An external URL is not ours to stream; point the client at it directly.
    let Some((drive_id, key)) = parse_reference(&stored) else {
        return match HeaderValue::from_str(&stored) {
            Ok(location) => (StatusCode::FOUND, [(LOCATION, location)]).into_response(),
            Err(_) => StatusCode::NOT_FOUND.into_response(),
        };
    };

    let drive = match sqlx::query_as::<_, DriveRow>("SELECT * FROM drives WHERE id = $1")
        .bind(drive_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "{what} Drive lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let operator = match open_operator(state, &drive) {
        Ok(operator) => operator,
        Err(error) => {
            tracing::error!(error = %error, "failed to open Drive to serve {what}");
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };

    // Images are bounded by MAX_IMAGE_BYTES on write, so reading the whole blob is
    // cheap and keeps the operator (and any SFTP keyfile it owns) alive for the read.
    match operator.read(&key).await {
        Ok(buffer) => {
            let mut response = Body::from(buffer.to_bytes()).into_response();
            let headers = response.headers_mut();
            headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_static(content_type_for_key(&key)),
            );
            // Uploads validate only the client-declared content type, so a stored blob
            // could be a polyglot. Forbid MIME sniffing so a browser can never render an
            // "image" as HTML/script (stored-XSS defense for user-uploaded media).
            headers.insert(
                "x-content-type-options",
                HeaderValue::from_static("nosniff"),
            );
            response
        }
        Err(error) if error.kind() == opendal::ErrorKind::NotFound => {
            StatusCode::NOT_FOUND.into_response()
        }
        Err(error) => {
            tracing::warn!(error = %error, "failed to read {what} blob");
            StatusCode::BAD_GATEWAY.into_response()
        }
    }
}

/// Resolves the default Drive an org's system blobs live on.
///
/// System files live on the default Drive of the org's canonical (oldest) project, the
/// one auto-created with the org. Selecting by `created_at` is deterministic and
/// survives project renames.
///
/// # Errors
/// Returns a ready `404` when the org has no default Drive (should not happen after the
/// boot reconcile) and `503` on a database error.
async fn default_drive_for_org(pool: &PgPool, org_id: Uuid) -> Result<DriveRow, Response> {
    match sqlx::query_as::<_, DriveRow>(
        "SELECT d.* FROM drives d
           JOIN projects p ON p.id = d.project_id
          WHERE p.org_id = $1 AND d.is_default
          ORDER BY p.created_at ASC
          LIMIT 1",
    )
    .bind(org_id)
    .fetch_optional(pool)
    .await
    {
        Ok(Some(row)) => Ok(row),
        Ok(None) => Err((
            StatusCode::CONFLICT,
            "the organization has no default Drive to store images on",
        )
            .into_response()),
        Err(error) => {
            tracing::error!(error = %error, "default Drive lookup failed");
            Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
    }
}

/// Builds a live operator for a Drive, opening its sealed credential.
///
/// # Errors
/// Returns an error when the stored config is malformed, the credential fails to open,
/// or the provider rejects the assembled configuration.
fn open_operator(state: &AppState, row: &DriveRow) -> eyre::Result<DriveOperator> {
    let config = row.parse_config()?;
    let secret = match row.secret.as_deref() {
        Some(blob) => Some(open_secret(&state.crypto, row.project_id, blob)?),
        None => None,
    };
    provider::operator(&config, secret.as_deref())
}

/// Reads a body stream into memory, failing with `413` once it exceeds `cap`.
async fn read_capped(body: Body, cap: u64) -> Result<Vec<u8>, Response> {
    let mut stream = body.into_data_stream();
    let mut buffer: Vec<u8> = Vec::new();
    while let Some(frame) = stream.next().await {
        let chunk = frame.map_err(|error| {
            tracing::warn!(error = %error, "image upload body stream failed");
            (StatusCode::BAD_REQUEST, "upload body stream failed").into_response()
        })?;
        if buffer.len() as u64 + chunk.len() as u64 > cap {
            return Err(too_large());
        }
        buffer.extend_from_slice(&chunk);
    }
    Ok(buffer)
}

/// Sets `column` to `reference` on the target user or org row.
async fn record_reference(
    pool: &PgPool,
    column: &str,
    target: &UpdateTarget,
    reference: &str,
) -> sqlx::Result<()> {
    // `column` is a fixed literal from the two call sites, never client input, so
    // formatting it into the statement is safe (there is no bind slot for a column).
    let (table, id) = match target {
        UpdateTarget::User(id) => ("users", *id),
        UpdateTarget::Org(id) => ("orgs", *id),
    };
    let statement = format!("UPDATE {table} SET {column} = $1 WHERE id = $2");
    sqlx::query(&statement)
        .bind(reference)
        .bind(id)
        .execute(pool)
        .await
        .map(|_| ())
}

/// Reads the current `column` value on the target user or org row.
async fn read_reference(
    pool: &PgPool,
    column: &str,
    target: &UpdateTarget,
) -> sqlx::Result<Option<String>> {
    // `column`/`table` are fixed literals from the two call sites, never client input.
    let (table, id) = match target {
        UpdateTarget::User(id) => ("users", *id),
        UpdateTarget::Org(id) => ("orgs", *id),
    };
    let statement = format!("SELECT {column} FROM {table} WHERE id = $1");
    sqlx::query_scalar::<_, String>(&statement)
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Reads a request's declared `Content-Length`, if present and well-formed.
fn declared_length(request: &Request) -> Option<u64> {
    request
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
}

/// The `413` response for an image over [`MAX_IMAGE_BYTES`].
fn too_large() -> Response {
    (
        StatusCode::PAYLOAD_TOO_LARGE,
        format!("image exceeds the {MAX_IMAGE_BYTES}-byte limit"),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Reference discrimination -------------------------------------------

    #[test]
    fn internal_reference_round_trips_through_parse() {
        let drive_id = Uuid::new_v4();
        let key = ".elysium/avatars/abc.png";
        let reference = internal_reference(drive_id, key);

        assert!(is_internal_reference(&reference));
        assert_eq!(
            parse_reference(&reference),
            Some((drive_id, key.to_string()))
        );
    }

    #[test]
    fn external_urls_are_not_internal_references() {
        for external in [
            "https://cdn.example.com/a.png",
            "http://example.test/logo.jpg",
            "",
        ] {
            assert!(!is_internal_reference(external));
            assert_eq!(parse_reference(external), None);
        }
    }

    #[test]
    fn malformed_internal_references_do_not_parse() {
        // A non-UUID authority, and a reference with no key after the Drive id.
        assert_eq!(parse_reference("elysium://not-a-uuid/key.png"), None);
        assert_eq!(
            parse_reference(&format!("elysium://{}/", Uuid::new_v4())),
            None
        );
        assert_eq!(
            parse_reference(&format!("elysium://{}", Uuid::new_v4())),
            None
        );
    }

    #[test]
    fn present_maps_internal_to_served_path_and_passes_external_through() {
        let user_id = Uuid::new_v4();
        let org_id = Uuid::new_v4();
        let internal_avatar = internal_reference(Uuid::new_v4(), ".elysium/avatars/x.png");
        let internal_logo = internal_reference(Uuid::new_v4(), ".elysium/logos/y.webp");

        assert_eq!(
            present_avatar_url(user_id, &internal_avatar),
            format!("/api/v1/users/{user_id}/avatar"),
        );
        assert_eq!(
            present_logo_url(org_id, &internal_logo),
            format!("/api/v1/orgs/{org_id}/logo"),
        );
        // External URLs and empty values are returned verbatim.
        assert_eq!(
            present_avatar_url(user_id, "https://cdn.example.com/a.png"),
            "https://cdn.example.com/a.png",
        );
        assert_eq!(present_logo_url(org_id, ""), "");
    }

    // --- Content-type allow-list --------------------------------------------

    #[test]
    fn allowed_image_types_map_to_canonical_extensions() {
        assert_eq!(extension_for("image/png"), Some("png"));
        assert_eq!(extension_for("image/jpeg"), Some("jpg"));
        assert_eq!(extension_for("image/gif"), Some("gif"));
        assert_eq!(extension_for("image/webp"), Some("webp"));
        // A charset parameter and odd casing do not defeat the match.
        assert_eq!(extension_for("IMAGE/PNG; charset=binary"), Some("png"));
    }

    #[test]
    fn disallowed_types_are_rejected() {
        // SVG is excluded on purpose (inline-script surface), as are non-images.
        for rejected in ["image/svg+xml", "text/html", "application/pdf", ""] {
            assert_eq!(extension_for(rejected), None);
        }
    }

    #[test]
    fn served_content_type_follows_the_stored_extension() {
        assert_eq!(content_type_for_key(".elysium/avatars/a.png"), "image/png");
        assert_eq!(content_type_for_key(".elysium/logos/b.jpg"), "image/jpeg");
        assert_eq!(content_type_for_key(".elysium/logos/c.webp"), "image/webp");
        // An unknown extension serves as a definite, non-executable type.
        assert_eq!(
            content_type_for_key(".elysium/avatars/d.bin"),
            "application/octet-stream",
        );
    }

    // --- Key derivation -----------------------------------------------------

    #[test]
    fn keys_live_under_the_reserved_hidden_prefix() {
        let user_id = Uuid::new_v4();
        let org_id = Uuid::new_v4();
        assert_eq!(
            format!("{AVATAR_PREFIX}/{user_id}.png"),
            format!(".elysium/avatars/{user_id}.png"),
        );
        assert_eq!(
            format!("{LOGO_PREFIX}/{org_id}.webp"),
            format!(".elysium/logos/{org_id}.webp"),
        );
        // The reserved segment matches what the file browser hides.
        assert!(format!("{AVATAR_PREFIX}/x").starts_with(super::super::files::RESERVED_PREFIX));
    }

    // --- Capped read --------------------------------------------------------

    #[tokio::test]
    async fn read_capped_accepts_at_the_limit_and_rejects_over_it() {
        let at_cap = read_capped(Body::from(vec![7u8; 5]), 5).await;
        assert_eq!(at_cap.expect("body at the cap is accepted"), vec![7u8; 5]);

        let over_cap = read_capped(Body::from(vec![0u8; 6]), 5).await;
        assert_eq!(
            over_cap.expect_err("an oversize body is refused").status(),
            StatusCode::PAYLOAD_TOO_LARGE,
        );
    }

    // --- DB-gated: upload writes a blob + sets the reference, read serves it -
    //
    // Ignored by default (needs a reachable postgres), like the other storage tests.
    // Run with `cargo test -- --ignored` and a DATABASE_URL. The Drive is a real
    // local-FS operator rooted at a tempdir, so the blob round-trips on disk; the org,
    // project, and Drive are committed under unique ids and dropped at the end.

    use std::sync::Arc;

    use axum::body::to_bytes;
    use opaque_ke::ServerSetup;
    use rand::rngs::OsRng;
    use sqlx::postgres::PgPoolOptions;

    use crate::crypto::Crypto;
    use crate::opaque::Suite;
    use crate::sse::SseRegistry;

    async fn connect_test_pool() -> PgPool {
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

    fn test_state(pool: PgPool) -> AppState {
        AppState {
            pool,
            sse: SseRegistry::default(),
            webauthn: crate::webauthn::build("localhost", "http://localhost:5173")
                .expect("the test WebAuthn RP builds"),
            opaque_setup: Arc::new(ServerSetup::<Suite>::new(&mut OsRng)),
            crypto: Crypto::new(&[0u8; 32]).expect("a 32-byte test key is valid"),
            redis_url: None,
            redis: None,
            http: reqwest::Client::new(),
            geoip_endpoint: None,
            rabbitmq_url: None,
            app_origin: "http://localhost:5173".to_string(),
            data_dir: "/srv/elysium".to_string(),
            provisioner: crate::orchestration::provisioner::ProvisionerHandle::in_memory(),
            routes: crate::orchestration::routing::RouteTable::new(),
        }
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn logo_upload_writes_blob_sets_reference_and_serves_it_while_external_is_untouched() {
        let pool = connect_test_pool().await;
        let state = test_state(pool.clone());

        // An org whose default Drive is a real local-FS operator at a tempdir.
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_str().expect("utf-8 temp path");
        let unique = Uuid::new_v4();
        let org = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO orgs (name, slug) VALUES ($1, $2) RETURNING id",
        )
        .bind("Branding Test")
        .bind(format!("branding-{unique}"))
        .fetch_one(&pool)
        .await
        .expect("insert org");
        let project = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO projects (org_id, name, slug) VALUES ($1, 'Default', $2) RETURNING id",
        )
        .bind(org)
        .bind(format!("default-{unique}"))
        .fetch_one(&pool)
        .await
        .expect("insert project");
        let drive = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO drives (project_id, org_id, name, kind, config, visibility, is_default)
             VALUES ($1, $2, 'Default', 1, $3, 1, TRUE) RETURNING id",
        )
        .bind(project)
        .bind(org)
        .bind(serde_json::json!({ "root": root }))
        .fetch_one(&pool)
        .await
        .expect("insert default FS drive");

        // Store an org logo through the shared pipeline.
        let png = b"\x89PNG\r\n\x1a\nfake-png-body".to_vec();
        let request = Request::builder()
            .header(CONTENT_TYPE, "image/png")
            .body(Body::from(png.clone()))
            .expect("build request");
        let served = store_image(
            &state,
            org,
            &format!("{LOGO_PREFIX}/{org}"),
            request,
            "logo_url",
            UpdateTarget::Org(org),
        )
        .await
        .expect("logo upload succeeds");
        assert_eq!(served, format!("/api/v1/orgs/{org}/logo"));

        // The column now holds an internal reference to the written blob.
        let stored = sqlx::query_scalar::<_, String>("SELECT logo_url FROM orgs WHERE id = $1")
            .bind(org)
            .fetch_one(&pool)
            .await
            .expect("read logo_url");
        let (ref_drive, key) = parse_reference(&stored).expect("stored an internal reference");
        assert_eq!(ref_drive, drive);
        assert_eq!(key, format!("{LOGO_PREFIX}/{org}.png"));

        // Serving the org logo streams the exact bytes back with the image type.
        let response = serve_logo(
            State(state.clone()),
            SessionUser(crate::session::Principal {
                sub: "tester".to_string(),
                email: String::new(),
                username: String::new(),
                active_org_id: Some(org),
            }),
            Path(org),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("image/png"),
        );
        let body = to_bytes(response.into_body(), MAX_IMAGE_BYTES as usize)
            .await
            .expect("collect body");
        assert_eq!(body.as_ref(), png.as_slice());

        // An external logo URL is left untouched and served as a redirect.
        sqlx::query("UPDATE orgs SET logo_url = $1 WHERE id = $2")
            .bind("https://cdn.example.com/logo.svg")
            .bind(org)
            .execute(&pool)
            .await
            .expect("set external logo");
        let redirect = serve_logo(
            State(state.clone()),
            SessionUser(crate::session::Principal {
                sub: "tester".to_string(),
                email: String::new(),
                username: String::new(),
                active_org_id: Some(org),
            }),
            Path(org),
        )
        .await;
        assert_eq!(redirect.status(), StatusCode::FOUND);
        assert_eq!(
            redirect
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("https://cdn.example.com/logo.svg"),
        );

        sqlx::query("DELETE FROM orgs WHERE id = $1")
            .bind(org)
            .execute(&pool)
            .await
            .expect("cleanup org");
    }
}
