//! Streaming file-transfer proxy: the per-Drive upload/download/list/mutate surface.
//!
//! File bytes move by **proxying through the backend, streaming** (see
//! `docs/storage.md`, Transfer). The client always talks to Elysium; the backend
//! streams bytes to and from the Drive's OpenDAL operator and never buffers a whole
//! object in memory. This is uniform across every provider and is the only thing that
//! works for FS and SFTP at all.
//!
//! Routes live under `/api/v1/drives/{id}` (wired in [`crate::api`]) and take the
//! in-Drive path as a URL-encoded `?path=` query parameter, except [`move_entry`]
//! which carries `from`/`to` in a protobuf [`MoveRequest`] body. Every route gates on
//! [`drives::can_access_drive`], the single storage access rule.
//!
//! # Path safety
//!
//! [`normalize_path`] cleans each client path: it strips the leading slash, collapses
//! empty segments, rejects any `..` traversal (`400`), and refuses the reserved
//! `.elysium/` system area (`404`, so the consumer-only tree stays hidden). Listings
//! filter that same prefix out, so it never appears in the file browser.
//!
//! # The upload cap
//!
//! Uploads enforce the per-org `max_upload_bytes` setting. A declared `Content-Length`
//! over the cap is rejected before the body is read; a stream that grows past the cap
//! mid-flight is aborted and its partial object discarded. Because transfers stream,
//! the cap is a policy choice, not a memory limit.

use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, Request, State};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_core::Stream;
use opendal::{Entry, Operator};
use serde::Deserialize;
use sqlx::PgPool;
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::proto::common::v1::Timestamp;
use crate::proto::storage::v1::{FileEntry, Listing, MoveRequest};
use crate::protobuf::Protobuf;
use crate::session::{Principal, SessionUser};
use crate::state::AppState;
use crate::storage::drives;
use crate::storage::provider::{self, DriveOperator};
use crate::users;

use super::{open_secret, DriveRow, DEFAULT_MAX_UPLOAD_BYTES};

/// The reserved system-file prefix, hidden from this public gateway.
///
/// Consumer data (avatars, org logos, git/registry/Astraval areas) lives under
/// `.elysium/`; this API neither reads nor writes it (see `docs/storage.md`).
pub(super) const RESERVED_PREFIX: &str = ".elysium";

/// Fallback content type for a download whose provider reports no MIME.
const OCTET_STREAM: &str = "application/octet-stream";

/// Chunk size for streamed uploads (8 MiB).
///
/// Bounds how much of an upload the backend buffers at once and, on S3, selects the
/// multipart part size so a large object uploads in parts rather than one request.
/// Large enough to keep S3 part counts sane, small enough that the backend never
/// holds a whole file.
const UPLOAD_CHUNK_BYTES: usize = 8 * 1024 * 1024;

// --- Request shapes ---------------------------------------------------------

/// The `?path=` query shared by the list, content, folder, and delete routes.
#[derive(Deserialize)]
pub struct PathQuery {
    /// The in-Drive path, URL-decoded by the query parser. Empty is the Drive root.
    #[serde(default)]
    path: String,
}

// --- Routes -----------------------------------------------------------------

/// `GET /api/v1/drives/{id}/files?path=`: list a folder as a protobuf [`Listing`].
///
/// Returns the direct children of `path` (the Drive root when empty). The reserved
/// `.elysium/` area is filtered out, and the directory's own self entry is dropped.
pub async fn list_files(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(drive_id): Path<Uuid>,
    Query(query): Query<PathQuery>,
) -> Response {
    let (row, _user_id) = match authorize(&state, &principal, drive_id).await {
        Ok(context) => context,
        Err(response) => return response,
    };
    let path = match normalize_path(&query.path) {
        Ok(path) => path,
        Err(response) => return response,
    };
    let drive = match open_operator(&state, &row) {
        Ok(drive) => drive,
        Err(response) => return response,
    };

    match list_dir(drive.operator(), &path).await {
        Ok(entries) => Protobuf(Listing {
            drive_id: drive_id.to_string(),
            path,
            entries,
        })
        .into_response(),
        Err(error) => opendal_response(&error),
    }
}

/// `GET /api/v1/drives/{id}/content?path=`: download an object, streamed.
///
/// Streams the object straight from the OpenDAL reader with an
/// `application/octet-stream` body (or the provider's MIME when known); the process
/// never holds the whole object. Listing a directory here is a `404`, since only a
/// file has content.
pub async fn download(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(drive_id): Path<Uuid>,
    Query(query): Query<PathQuery>,
) -> Response {
    let (row, _user_id) = match authorize(&state, &principal, drive_id).await {
        Ok(context) => context,
        Err(response) => return response,
    };
    let path = match normalize_path(&query.path) {
        Ok(path) if !path.is_empty() => path,
        Ok(_) => return (StatusCode::BAD_REQUEST, "path is required").into_response(),
        Err(response) => return response,
    };
    let drive = match open_operator(&state, &row) {
        Ok(drive) => drive,
        Err(response) => return response,
    };

    let metadata = match drive.stat(&path).await {
        Ok(metadata) => metadata,
        Err(error) => return opendal_response(&error),
    };
    if metadata.is_dir() {
        return StatusCode::NOT_FOUND.into_response();
    }

    let reader = match drive.reader(&path).await {
        Ok(reader) => reader,
        Err(error) => return opendal_response(&error),
    };
    let stream = match reader.into_bytes_stream(..).await {
        Ok(stream) => stream,
        Err(error) => return opendal_response(&error),
    };

    let content_type = match metadata.content_type() {
        Some(mime) if !mime.is_empty() => HeaderValue::from_str(mime),
        _ => Ok(HeaderValue::from_static(OCTET_STREAM)),
    };
    let content_type = content_type.unwrap_or_else(|_| HeaderValue::from_static(OCTET_STREAM));

    // The download outlives this handler, so the operator (and any SFTP keyfile it
    // owns) rides along in the stream to stay alive until the last byte is sent.
    let body = Body::from_stream(GuardedDownload {
        _drive: drive,
        inner: Box::pin(stream),
    });
    let mut response = Response::new(body);
    response.headers_mut().insert(CONTENT_TYPE, content_type);
    if let Ok(length) = HeaderValue::from_str(&metadata.content_length().to_string()) {
        response.headers_mut().insert(CONTENT_LENGTH, length);
    }
    response
}

/// `PUT /api/v1/drives/{id}/content?path=`: upload an object, streamed.
///
/// Streams the request body into the OpenDAL writer in [`UPLOAD_CHUNK_BYTES`] chunks
/// (multipart on S3), enforcing the per-org `max_upload_bytes` cap: a declared
/// `Content-Length` over the cap is a `413` before the body is read, and a body that
/// grows past the cap is aborted mid-stream. Returns `204` on success.
pub async fn upload(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(drive_id): Path<Uuid>,
    Query(query): Query<PathQuery>,
    request: Request,
) -> Response {
    let (row, _user_id) = match authorize(&state, &principal, drive_id).await {
        Ok(context) => context,
        Err(response) => return response,
    };
    let path = match normalize_path(&query.path) {
        Ok(path) if !path.is_empty() => path,
        Ok(_) => return (StatusCode::BAD_REQUEST, "path is required").into_response(),
        Err(response) => return response,
    };
    let cap = match max_upload_bytes(&state.pool, row.org_id).await {
        Ok(cap) => cap,
        Err(response) => return response,
    };

    // Reject an oversize upload up front when the client declares its length, so a
    // doomed transfer never touches the provider.
    if let Some(declared) = declared_length(&request) {
        if declared > cap {
            return payload_too_large(cap);
        }
    }

    let drive = match open_operator(&state, &row) {
        Ok(drive) => drive,
        Err(response) => return response,
    };

    let body = request.into_body().into_data_stream();
    match stream_to_drive(drive.operator(), &path, body, cap).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(UploadError::TooLarge) => payload_too_large(cap),
        Err(UploadError::Body(error)) => {
            tracing::warn!(error = %error, "upload body stream failed");
            (StatusCode::BAD_REQUEST, "upload body stream failed").into_response()
        }
        Err(UploadError::Provider(error)) => opendal_response(&error),
    }
}

/// `POST /api/v1/drives/{id}/folders?path=`: create a folder. Returns `204`.
pub async fn create_folder(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(drive_id): Path<Uuid>,
    Query(query): Query<PathQuery>,
) -> Response {
    let (row, _user_id) = match authorize(&state, &principal, drive_id).await {
        Ok(context) => context,
        Err(response) => return response,
    };
    let path = match normalize_path(&query.path) {
        Ok(path) if !path.is_empty() => path,
        Ok(_) => return (StatusCode::BAD_REQUEST, "path is required").into_response(),
        Err(response) => return response,
    };
    let drive = match open_operator(&state, &row) {
        Ok(drive) => drive,
        Err(response) => return response,
    };

    // OpenDAL's create_dir requires a trailing slash to denote a directory.
    match drive.create_dir(&format!("{path}/")).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => opendal_response(&error),
    }
}

/// `POST /api/v1/drives/{id}/move`: move or rename an entry. Returns `204`.
///
/// Takes a protobuf [`MoveRequest`] with `from`/`to`; both are normalized and neither
/// may touch the reserved prefix or escape the Drive root.
pub async fn move_entry(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(drive_id): Path<Uuid>,
    Protobuf(request): Protobuf<MoveRequest>,
) -> Response {
    let (row, _user_id) = match authorize(&state, &principal, drive_id).await {
        Ok(context) => context,
        Err(response) => return response,
    };
    let from = match normalize_path(&request.from) {
        Ok(path) if !path.is_empty() => path,
        Ok(_) => return (StatusCode::BAD_REQUEST, "from is required").into_response(),
        Err(response) => return response,
    };
    let to = match normalize_path(&request.to) {
        Ok(path) if !path.is_empty() => path,
        Ok(_) => return (StatusCode::BAD_REQUEST, "to is required").into_response(),
        Err(response) => return response,
    };
    let drive = match open_operator(&state, &row) {
        Ok(drive) => drive,
        Err(response) => return response,
    };

    match drive.rename(&from, &to).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => opendal_response(&error),
    }
}

/// `DELETE /api/v1/drives/{id}/files?path=`: delete an entry. Returns `204`.
pub async fn delete_file(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(drive_id): Path<Uuid>,
    Query(query): Query<PathQuery>,
) -> Response {
    let (row, _user_id) = match authorize(&state, &principal, drive_id).await {
        Ok(context) => context,
        Err(response) => return response,
    };
    let path = match normalize_path(&query.path) {
        Ok(path) if !path.is_empty() => path,
        Ok(_) => return (StatusCode::BAD_REQUEST, "path is required").into_response(),
        Err(response) => return response,
    };
    let drive = match open_operator(&state, &row) {
        Ok(drive) => drive,
        Err(response) => return response,
    };

    match drive.delete(&path).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => opendal_response(&error),
    }
}

// --- Streaming primitives ---------------------------------------------------

/// A download stream that keeps its Drive operator alive until the last byte is sent.
///
/// [`Body::from_stream`] outlives the handler, so the [`DriveOperator`] (and the SFTP
/// keyfile it may own) must travel with the byte stream rather than drop when the
/// handler returns. The type is `Unpin` because both fields are, so polling forwards
/// straight to the boxed inner stream.
struct GuardedDownload {
    _drive: DriveOperator,
    inner: Pin<Box<dyn Stream<Item = std::io::Result<Bytes>> + Send>>,
}

impl Stream for GuardedDownload {
    type Item = std::io::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().inner.as_mut().poll_next(cx)
    }
}

/// Why a streamed upload could not be committed to the provider.
#[derive(Debug)]
enum UploadError {
    /// The body grew past the per-org cap; the partial object was discarded.
    TooLarge,
    /// The client's request-body stream failed mid-transfer.
    Body(Box<dyn std::error::Error + Send + Sync>),
    /// The provider rejected a write.
    Provider(opendal::Error),
}

/// Streams a body into a Drive object, enforcing a hard byte ceiling.
///
/// Writes each frame to the OpenDAL writer in order, tracking the running total; once
/// it would exceed `cap` the writer is aborted (discarding the partial object) and
/// [`UploadError::TooLarge`] is returned. Returns the number of bytes written.
///
/// Each read and write is an await point, so this copy loop yields to the runtime on
/// every chunk (rust.md M-YIELD-POINTS); no explicit yield is needed.
///
/// # Errors
/// Returns [`UploadError::TooLarge`] when the body exceeds `cap`, [`UploadError::Body`]
/// when the client stream fails, and [`UploadError::Provider`] when a write fails.
async fn stream_to_drive<S, E>(
    operator: &Operator,
    path: &str,
    mut body: S,
    cap: u64,
) -> Result<u64, UploadError>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let mut writer = operator
        .writer_with(path)
        .chunk(UPLOAD_CHUNK_BYTES)
        .await
        .map_err(UploadError::Provider)?;

    let mut written: u64 = 0;
    while let Some(frame) = body.next().await {
        let chunk = frame.map_err(|error| UploadError::Body(error.into()))?;
        written = written.saturating_add(chunk.len() as u64);
        if written > cap {
            // Abort the writer, then best-effort delete: some backends (local FS)
            // create the target on open and cannot abort it away, so a rejected upload
            // must not leave a partial object behind.
            let _ = writer.abort().await;
            let _ = operator.delete(path).await;
            return Err(UploadError::TooLarge);
        }
        writer.write(chunk).await.map_err(UploadError::Provider)?;
    }

    writer.close().await.map_err(UploadError::Provider)?;
    Ok(written)
}

/// Lists a directory's direct children, hiding the reserved area and the self entry.
///
/// # Errors
/// Returns the provider error when the listing fails.
async fn list_dir(operator: &Operator, dir: &str) -> opendal::Result<Vec<FileEntry>> {
    // OpenDAL lists a directory by its trailing-slash path; empty is the Drive root.
    let listing_path = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };

    let entries = operator.list(&listing_path).await?;
    let mut files = Vec::with_capacity(entries.len());
    for entry in entries {
        // Drop the directory's own self entry.
        if entry.path() == listing_path {
            continue;
        }
        // Hide the reserved system area wherever it surfaces.
        if top_segment(entry.path()) == RESERVED_PREFIX {
            continue;
        }
        files.push(file_entry(&entry));
    }
    Ok(files)
}

/// Builds a protobuf [`FileEntry`] from an OpenDAL directory entry.
///
/// Shared with the federated-search surface ([`crate::storage::search`]), which turns
/// each matching walked entry into the same wire shape a listing uses.
pub(super) fn file_entry(entry: &Entry) -> FileEntry {
    let metadata = entry.metadata();
    FileEntry {
        name: entry.name().trim_end_matches('/').to_string(),
        path: entry.path().to_string(),
        is_dir: metadata.is_dir(),
        size: metadata.content_length(),
        // OpenDAL's last-modified is its own jiff-backed Timestamp; convert through
        // std `SystemTime` (a provided `From` impl) into the canonical UTC proto
        // Timestamp so nothing here depends on opendal's internal time type.
        modified: metadata
            .last_modified()
            .map(std::time::SystemTime::from)
            .map(proto_timestamp),
        mime: metadata.content_type().unwrap_or_default().to_string(),
    }
}

/// Builds the canonical UTC proto [`Timestamp`] from a `SystemTime` instant.
///
/// `nanos` is always in `[0, 1_000_000_000)`, borrowing a second for pre-epoch
/// instants so the fraction never goes negative (see docs/proto.md).
fn proto_timestamp(instant: std::time::SystemTime) -> Timestamp {
    let (epoch_seconds, nanos) = match instant.duration_since(std::time::UNIX_EPOCH) {
        Ok(elapsed) => (elapsed.as_secs() as i64, elapsed.subsec_nanos()),
        Err(before_epoch) => {
            let elapsed = before_epoch.duration();
            if elapsed.subsec_nanos() == 0 {
                (-(elapsed.as_secs() as i64), 0)
            } else {
                // Borrow a whole second so the nanosecond remainder stays positive.
                (
                    -(elapsed.as_secs() as i64) - 1,
                    1_000_000_000 - elapsed.subsec_nanos(),
                )
            }
        }
    };
    Timestamp {
        epoch_seconds,
        nanos,
        timezone: "Etc/UTC".to_string(),
    }
}

// --- Path handling ----------------------------------------------------------

/// Normalizes a client path, rejecting traversal and the reserved system prefix.
///
/// Returns the cleaned, provider-rooted path: no leading slash, no empty or `.`
/// segments, and no `..`. The empty string is the Drive root.
///
/// # Errors
/// Returns a ready `400` when the path contains a `..` segment, and a `404` when it
/// touches the reserved `.elysium/` area (kept hidden rather than acknowledged).
#[expect(
    clippy::result_large_err,
    reason = "the Err is a ready HTTP Response the caller returns as-is"
)]
fn normalize_path(raw: &str) -> Result<String, Response> {
    let mut segments: Vec<&str> = Vec::new();
    for segment in raw.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                return Err((StatusCode::BAD_REQUEST, "path may not contain '..'").into_response());
            }
            other => segments.push(other),
        }
    }
    if segments.first() == Some(&RESERVED_PREFIX) {
        return Err(StatusCode::NOT_FOUND.into_response());
    }
    Ok(segments.join("/"))
}

/// Returns the first path segment, e.g. `".elysium"` for `".elysium/git/repo"`.
pub(super) fn top_segment(path: &str) -> &str {
    path.trim_start_matches('/').split('/').next().unwrap_or("")
}

// --- Shared helpers ---------------------------------------------------------

/// Loads a Drive and confirms the caller may reach it, the gate every route shares.
///
/// # Errors
/// Returns a ready response: `404` when the Drive is unknown, `401` when the session
/// does not resolve to a user, `403` when the caller cannot access the Drive, and
/// `503` on a database error.
async fn authorize(
    state: &AppState,
    principal: &Principal,
    drive_id: Uuid,
) -> Result<(DriveRow, Uuid), Response> {
    let row = drives::load_drive(state, drive_id).await?;
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return Err(StatusCode::UNAUTHORIZED.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "drive transfer user lookup failed");
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };
    let mut conn = match state.pool.acquire().await {
        Ok(conn) => conn,
        Err(error) => {
            tracing::error!(error = %error, "drive transfer connection acquire failed");
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };
    match drives::can_access_drive(&mut conn, user_id, &row).await {
        Ok(true) => Ok((row, user_id)),
        Ok(false) => Err(StatusCode::FORBIDDEN.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "drive access resolution failed");
            Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
    }
}

/// Builds a live OpenDAL operator for a Drive, opening its sealed credential.
///
/// # Errors
/// Returns a ready `500` when the stored config or credential is unusable, and a `502`
/// when the provider configuration is rejected.
#[expect(
    clippy::result_large_err,
    reason = "the Err is a ready HTTP Response the caller returns as-is"
)]
fn open_operator(state: &AppState, row: &DriveRow) -> Result<DriveOperator, Response> {
    let config = row.parse_config().map_err(|error| {
        tracing::error!(error = %error, "stored drive config is malformed");
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;
    let secret = match row.secret.as_deref() {
        Some(blob) => match open_secret(&state.crypto, row.project_id, blob) {
            Ok(secret) => Some(secret),
            Err(error) => {
                tracing::error!(error = %error, "drive credential failed to open");
                return Err(StatusCode::INTERNAL_SERVER_ERROR.into_response());
            }
        },
        None => None,
    };
    provider::operator(&config, secret.as_deref()).map_err(|error| {
        tracing::error!(error = %error, "failed to build drive operator");
        StatusCode::BAD_GATEWAY.into_response()
    })
}

/// Resolves the per-org upload cap in bytes, defaulting when the org has no row.
///
/// # Errors
/// Returns a ready `503` when the settings lookup fails.
async fn max_upload_bytes(pool: &PgPool, org_id: Uuid) -> Result<u64, Response> {
    let configured = sqlx::query_scalar::<_, i64>(
        "SELECT max_upload_bytes FROM org_storage_settings WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        tracing::error!(error = %error, "upload cap lookup failed");
        StatusCode::SERVICE_UNAVAILABLE.into_response()
    })?;
    // No row means the org uses the default cap (the migration's column default).
    Ok(configured.unwrap_or(DEFAULT_MAX_UPLOAD_BYTES).max(0) as u64)
}

/// Reads a request's declared `Content-Length`, if present and well-formed.
fn declared_length(request: &Request) -> Option<u64> {
    request
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
}

/// The `413` response for an upload that exceeds the per-org cap.
fn payload_too_large(cap: u64) -> Response {
    (
        StatusCode::PAYLOAD_TOO_LARGE,
        format!("upload exceeds the per-file limit of {cap} bytes"),
    )
        .into_response()
}

/// Maps an OpenDAL error onto a ready HTTP response.
///
/// A missing entry is `404` and a denied one is `403`; any other provider failure is a
/// `502`, since the fault lies with the upstream Drive, not the client.
fn opendal_response(error: &opendal::Error) -> Response {
    use opendal::ErrorKind;
    match error.kind() {
        ErrorKind::NotFound => StatusCode::NOT_FOUND.into_response(),
        ErrorKind::PermissionDenied => StatusCode::FORBIDDEN.into_response(),
        _ => {
            tracing::warn!(error = %error, "drive provider operation failed");
            StatusCode::BAD_GATEWAY.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{DriveConfig, FsConfig};

    /// Builds a live FS operator rooted at `root` for the byte-level tests.
    fn fs_operator(root: &str) -> DriveOperator {
        provider::operator(
            &DriveConfig::Fs(FsConfig {
                root: root.to_string(),
            }),
            None,
        )
        .expect("build FS operator")
    }

    /// Collects a fresh download stream for `path` into a byte vector.
    async fn download_bytes(operator: &Operator, path: &str) -> Vec<u8> {
        let reader = operator.reader(path).await.expect("open reader");
        let mut stream = Box::pin(reader.into_bytes_stream(..).await.expect("byte stream"));
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            bytes.extend_from_slice(&chunk.expect("read chunk"));
        }
        bytes
    }

    /// A multi-chunk request body, so the copy loop is exercised across frames.
    fn chunked_body(chunks: &[&'static [u8]]) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
        let frames: Vec<Result<Bytes, std::io::Error>> = chunks
            .iter()
            .map(|chunk| Ok(Bytes::from_static(chunk)))
            .collect();
        tokio_stream::iter(frames)
    }

    // --- Path normalization -------------------------------------------------

    #[test]
    fn normalize_path_cleans_slashes_and_dot_segments() {
        assert_eq!(normalize_path("foo/bar").unwrap(), "foo/bar");
        assert_eq!(normalize_path("/foo//bar/").unwrap(), "foo/bar");
        assert_eq!(normalize_path("./foo/./bar").unwrap(), "foo/bar");
        assert_eq!(normalize_path("").unwrap(), "");
        assert_eq!(normalize_path("/").unwrap(), "");
    }

    #[test]
    fn normalize_path_rejects_traversal_with_400() {
        let response = normalize_path("foo/../../etc/passwd").expect_err("traversal rejected");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let response = normalize_path("..").expect_err("bare .. rejected");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn normalize_path_hides_the_reserved_prefix_with_404() {
        for reserved in [".elysium", ".elysium/git/repo", "/.elysium/logo.png"] {
            let response = normalize_path(reserved).expect_err("reserved refused");
            assert_eq!(
                response.status(),
                StatusCode::NOT_FOUND,
                "{reserved} must be hidden",
            );
        }
        // A path that merely resembles the prefix is allowed through.
        assert_eq!(normalize_path(".elysiumx/file").unwrap(), ".elysiumx/file");
    }

    // --- FS round trip: upload -> list -> download -> mkdir -> move -> delete

    #[tokio::test]
    async fn fs_drive_round_trips_every_operation() {
        let dir = tempfile::tempdir().expect("temp dir");
        let operator = fs_operator(dir.path().to_str().expect("utf-8 temp path"));
        let operator = operator.operator();

        // Upload, delivered as several frames to prove the streamed copy path.
        let written = stream_to_drive(
            operator,
            "docs/hello.txt",
            chunked_body(&[b"hello ", b"streamed ", b"world"]),
            1_000,
        )
        .await
        .expect("upload succeeds");
        assert_eq!(written, "hello streamed world".len() as u64);

        // List reflects the upload under its folder.
        let listing = list_dir(operator, "docs").await.expect("list docs");
        assert!(
            listing
                .iter()
                .any(|entry| entry.name == "hello.txt" && !entry.is_dir),
            "the uploaded file appears in its folder",
        );

        // Download is byte-for-byte.
        assert_eq!(
            download_bytes(operator, "docs/hello.txt").await,
            b"hello streamed world",
        );

        // mkdir then list shows the new folder.
        operator.create_dir("docs/sub/").await.expect("mkdir");
        let listing = list_dir(operator, "docs").await.expect("list docs again");
        assert!(
            listing
                .iter()
                .any(|entry| entry.name == "sub" && entry.is_dir),
            "the new folder appears in the listing",
        );

        // Move renames the object; the old path is gone, the new one present.
        operator
            .rename("docs/hello.txt", "docs/renamed.txt")
            .await
            .expect("move");
        assert!(!operator.exists("docs/hello.txt").await.expect("exists old"));
        assert_eq!(
            download_bytes(operator, "docs/renamed.txt").await,
            b"hello streamed world",
        );

        // Delete removes the object; the listing no longer surfaces it.
        operator.delete("docs/renamed.txt").await.expect("delete");
        let listing = list_dir(operator, "docs").await.expect("list after delete");
        assert!(
            !listing.iter().any(|entry| entry.name == "renamed.txt"),
            "the deleted file is gone from the listing",
        );
    }

    // --- Upload cap ---------------------------------------------------------

    #[tokio::test]
    async fn upload_at_the_cap_succeeds_and_over_it_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let operator = fs_operator(dir.path().to_str().expect("utf-8 temp path"));
        let operator = operator.operator();

        // A body exactly at the cap is written whole.
        let written = stream_to_drive(operator, "at-cap.bin", chunked_body(&[b"12345"]), 5)
            .await
            .expect("body at the cap succeeds");
        assert_eq!(written, 5);

        // A body one byte over the cap is rejected, and nothing is committed.
        let result =
            stream_to_drive(operator, "over-cap.bin", chunked_body(&[b"123", b"456"]), 5).await;
        assert!(
            matches!(result, Err(UploadError::TooLarge)),
            "an oversize upload is refused",
        );
        assert!(
            !operator.exists("over-cap.bin").await.expect("exists check"),
            "a rejected upload leaves no object behind",
        );
    }

    // --- Reserved area + self entry filtering -------------------------------

    #[tokio::test]
    async fn listing_hides_the_reserved_area() {
        let dir = tempfile::tempdir().expect("temp dir");
        let operator = fs_operator(dir.path().to_str().expect("utf-8 temp path"));
        let operator = operator.operator();

        // Write a normal file at the root and a file inside the reserved area.
        operator
            .write("visible.txt", "hi".as_bytes().to_vec())
            .await
            .expect("write visible");
        operator
            .write(".elysium/avatars/a.png", vec![0u8; 4])
            .await
            .expect("write reserved");

        let listing = list_dir(operator, "").await.expect("list root");
        assert!(listing.iter().any(|entry| entry.name == "visible.txt"));
        assert!(
            !listing.iter().any(|entry| entry.name == RESERVED_PREFIX),
            "the reserved system area never appears in a listing",
        );
    }

    // --- End-to-end access gate ---------------------------------------------
    //
    // Ignored by default (needs a reachable postgres), like the DB-backed tests in
    // `drives.rs`. Unlike those, `authorize` reads through the pool directly, so this
    // test cannot roll back a transaction; it commits under unique ids and deletes the
    // org (cascading its rows) at the end.

    use std::sync::Arc;

    use opaque_ke::ServerSetup;
    use rand::rngs::OsRng;
    use sqlx::PgConnection;

    use crate::crypto::Crypto;
    use crate::opaque::Suite;
    use crate::projects::{add_member, ProjectRole};
    use crate::sse::SseRegistry;
    use crate::storage::{DriveKind, DriveVisibility};

    /// Builds an [`AppState`] around a real pool for the end-to-end authorize test.
    fn test_state_with_pool(pool: PgPool) -> AppState {
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
        }
    }

    async fn connect_test_pool() -> PgPool {
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

    /// Inserts a user and returns its id and subject (the session's `sub`).
    async fn insert_user(conn: &mut PgConnection, tag: &str) -> (Uuid, String) {
        let subject = format!("{tag}-{}", Uuid::new_v4());
        let id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO users (subject, email) VALUES ($1, $2) RETURNING id",
        )
        .bind(&subject)
        .bind(format!("{subject}@example.test"))
        .fetch_one(conn)
        .await
        .expect("insert user");
        (id, subject)
    }

    fn principal(subject: &str) -> Principal {
        Principal {
            sub: subject.to_string(),
            email: String::new(),
            username: String::new(),
            active_org_id: None,
        }
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn authorize_denies_a_member_without_drive_access() {
        let pool = connect_test_pool().await;
        let state = test_state_with_pool(pool.clone());
        let mut conn = pool.acquire().await.expect("acquire");

        let unique = Uuid::new_v4();
        let org = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO orgs (name, slug) VALUES ($1, $2) RETURNING id",
        )
        .bind("Files Gate")
        .bind(format!("files-gate-{unique}"))
        .fetch_one(&mut *conn)
        .await
        .expect("insert org");
        let project = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO projects (org_id, name, slug) VALUES ($1, $2, $2) RETURNING id",
        )
        .bind(org)
        .bind(format!("proj-{unique}"))
        .fetch_one(&mut *conn)
        .await
        .expect("insert project");
        let drive = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO drives (project_id, org_id, name, kind, config, visibility)
             VALUES ($1, $2, 'Locked', $3, $4, $5) RETURNING id",
        )
        .bind(project)
        .bind(org)
        .bind(DriveKind::Fs.as_db())
        .bind(serde_json::json!({ "root": "/srv/data" }))
        .bind(DriveVisibility::Restricted.as_db())
        .fetch_one(&mut *conn)
        .await
        .expect("insert restricted drive");

        // A plain member with project access but no drive grant is denied with 403.
        let (member_id, member_sub) = insert_user(&mut conn, "member").await;
        sqlx::query("INSERT INTO org_members (user_id, org_id, role) VALUES ($1, $2, 'member')")
            .bind(member_id)
            .bind(org)
            .execute(&mut *conn)
            .await
            .expect("add org member");
        add_member(&mut conn, project, member_id, ProjectRole::Member)
            .await
            .expect("add project member");

        let denied = authorize(&state, &principal(&member_sub), drive)
            .await
            .expect_err("a member without a grant is denied");
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);

        // An org admin bypasses to every Drive, restricted included.
        let (admin_id, admin_sub) = insert_user(&mut conn, "admin").await;
        sqlx::query("INSERT INTO org_members (user_id, org_id, role) VALUES ($1, $2, 'admin')")
            .bind(admin_id)
            .bind(org)
            .execute(&mut *conn)
            .await
            .expect("add org admin");
        let (row, resolved) = authorize(&state, &principal(&admin_sub), drive)
            .await
            .expect("an org admin reaches every Drive");
        assert_eq!(row.id, drive);
        assert_eq!(resolved, admin_id);

        // An unknown Drive is a 404 regardless of who asks.
        let missing = authorize(&state, &principal(&admin_sub), Uuid::new_v4())
            .await
            .expect_err("an unknown Drive is not found");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);

        // Clean up: deleting the org cascades to its projects, drives, and members.
        sqlx::query("DELETE FROM orgs WHERE id = $1")
            .bind(org)
            .execute(&pool)
            .await
            .expect("cleanup org");
    }
}
