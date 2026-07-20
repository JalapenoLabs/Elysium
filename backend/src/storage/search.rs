//! Live, federated, streaming file search across a caller's accessible Drives.
//!
//! Search is a gateway concern, not an index (see `docs/storage.md`, Search). There is
//! no persistent metadata mirror: every request fans out **concurrently** to each
//! target Drive, walks it through the provider's own OpenDAL operator, and **streams**
//! matches back per source as they resolve. A Postgres mirror was rejected for the
//! duplication, bloat, and staleness it would bring at tens of millions of files.
//!
//! # The wire contract
//!
//! `POST /api/v1/search` takes a protobuf [`SearchRequest`] and answers with a single
//! chunked body of **length-delimited [`SearchFrame`] messages**: each frame is written
//! with [`prost::Message::encode_length_delimited_to_vec`], a protobuf varint byte
//! length followed by that many message bytes. A client frames the stream by reading a
//! varint length, then that many bytes, repeatedly, decoding each with
//! `SearchFrame::decode_length_delimited`. Each Drive emits `SEARCHING` first, then zero
//! or more `SEARCHING` frames carrying match batches, then a terminal `DONE` (or `SLOW`
//! while it keeps going, or `ERROR` with a message). A fast Drive's frames reach the
//! client before a slow Drive finishes, because sources run concurrently and share one
//! channel rather than being collected first.
//!
//! # Scope, matching, and hiding
//!
//! When `drive_ids` is empty the search covers every Drive the caller can reach across
//! their accessible projects in the active org, gated by
//! [`drives::can_access_drive`], the single storage access rule; an explicit
//! `drive_ids` list is intersected with that same rule, so an unreachable Drive is
//! silently skipped rather than leaked. Matching is a case-insensitive substring test
//! over each entry's provider-rooted path (filename/path only in v1, no content search),
//! and the reserved `.elysium/` system area is filtered out just as it is from listings.
//!
//! # Concurrency, cancellation, timeouts
//!
//! Sources run under a bounded [`Semaphore`] ([`MAX_CONCURRENT_DRIVE_SEARCHES`]) so a
//! large org never spawns unbounded provider walks. The fan-out tasks are owned by the
//! response body ([`SearchBody`]) through a [`JoinSet`], so when the client disconnects
//! and the body drops, every in-flight provider walk is aborted (rust.md
//! M-YIELD-POINTS keeps each walk cooperative). A source that passes [`SLOW_AFTER`]
//! emits one `SLOW` frame and keeps going; one that passes [`PER_SOURCE_TIMEOUT`] emits
//! `ERROR` and stops, so a slow or failing Drive never blocks the others.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_core::Stream;
use opendal::{Entry, Operator};
use prost::Message;
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinSet;
use tokio::time::Instant;
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::crypto::Crypto;
use crate::proto::common::v1::Uuid as ProtoUuid;
use crate::proto::storage::v1::{search_frame::Status, FileEntry, SearchFrame, SearchRequest};
use crate::protobuf::Protobuf;
use crate::session::SessionUser;
use crate::state::AppState;
use crate::storage::drives::can_access_drive;
use crate::storage::provider::{self, DriveOperator};
use crate::storage::{files, open_secret, DriveRow};
use crate::users;

/// How many Drives may walk their provider at once (bounded fan-out).
///
/// Large enough that a handful of Drives all light up together, small enough that a
/// deployment with many Drives never opens an unbounded number of concurrent provider
/// connections (rust.md M-THROUGHPUT). A source waits for a permit before walking.
const MAX_CONCURRENT_DRIVE_SEARCHES: usize = 8;

/// Frames buffered between the fan-out tasks and the client before backpressure.
///
/// A slow client throttles the walks (each frame send awaits capacity) rather than
/// letting the backend accumulate unbounded matches in memory.
const FRAME_CHANNEL_CAPACITY: usize = 32;

/// Matches accumulated before a `SEARCHING` frame flushes them to the client.
///
/// Bounds how long the client waits to see progress on a match-heavy Drive without
/// sending a frame per hit.
const MATCH_BATCH_SIZE: usize = 64;

/// How long a source may run before it emits a single `SLOW` frame and keeps going.
///
/// Signals the UI to attribute a spinner to this specific source; results may still
/// arrive. A genuine S3 name search is slow because S3 has no server-side name search.
const SLOW_AFTER: Duration = Duration::from_secs(5);

/// Hard per-source ceiling; on exceeding it a source emits `ERROR` and stops.
///
/// Keeps one wedged provider from holding a fan-out permit forever and starving the
/// others. Generous, since a large bucket walk is legitimately slow.
const PER_SOURCE_TIMEOUT: Duration = Duration::from_secs(60);

/// Entries walked between cooperative yields on a fast, match-poor provider.
///
/// The provider lister's `next().await` is the usual yield point, but a local FS that
/// returns entries without real I/O could otherwise monopolize the runtime; yielding
/// periodically keeps the fan-out fair (rust.md M-YIELD-POINTS).
const YIELD_EVERY: usize = 256;

/// Content type of the streamed length-delimited `SearchFrame` body.
///
/// A raw byte stream of concatenated length-delimited protobuf messages; the framing,
/// not a MIME subtype, tells the client where each frame ends (see the module docs).
const SEARCH_STREAM_CONTENT_TYPE: &str = "application/octet-stream";

/// `POST /api/v1/search`: fan out across accessible Drives, streaming frames per source.
///
/// The body is a [`SearchRequest`]; the response is a chunked stream of length-delimited
/// [`SearchFrame`] messages (see the module docs for the framing). An empty query is a
/// `400`; an absent active org is a `400`. Drives the caller cannot access are dropped
/// from the scope rather than reported, so the stream reveals only reachable sources.
pub async fn search(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Protobuf(request): Protobuf<SearchRequest>,
) -> Response {
    let query = request.query.trim();
    if query.is_empty() {
        return (StatusCode::BAD_REQUEST, "query is required").into_response();
    }
    // Normalize once: matching lowercases each entry path against this.
    let query = query.to_lowercase();

    let Some(org_id) = principal.active_org_id else {
        return (StatusCode::BAD_REQUEST, "no active organization").into_response();
    };
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "search user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let drives = match resolve_target_drives(&state, user_id, org_id, &request.drive_ids).await {
        Ok(drives) => drives,
        Err(response) => return response,
    };

    // Build one source per Drive; each opens its own operator inside the task so a bad
    // config or credential becomes that source's ERROR frame, not a failed request.
    let mut builders: Vec<SourceBuilder> = Vec::with_capacity(drives.len());
    for row in drives {
        let crypto = state.crypto.clone();
        let query = query.clone();
        builders.push(Box::new(move |sink: FrameSink| {
            Box::pin(search_one_drive(sink, crypto, row, query)) as SourceFuture
        }));
    }

    let body = stream_frames(builders, MAX_CONCURRENT_DRIVE_SEARCHES);
    let mut response = Response::new(Body::from_stream(body));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static(SEARCH_STREAM_CONTENT_TYPE),
    );
    response
}

/// Resolves the accessible Drives to search, applying [`can_access_drive`] to each.
///
/// An empty `requested` covers every Drive in the active org whose project the caller
/// can reach; a non-empty list is confined to those ids. Either way the access rule is
/// the final gate, so cross-project and cross-org Drives never enter the scope.
///
/// # Errors
/// Returns a ready `503` response when acquiring a connection or a query fails.
async fn resolve_target_drives(
    state: &AppState,
    user_id: Uuid,
    org_id: Uuid,
    requested: &[ProtoUuid],
) -> Result<Vec<DriveRow>, Response> {
    let mut conn = state.pool.acquire().await.map_err(|error| {
        tracing::error!(error = %error, "search connection acquire failed");
        StatusCode::SERVICE_UNAVAILABLE.into_response()
    })?;

    let rows = if requested.is_empty() {
        sqlx::query_as::<_, DriveRow>(
            "SELECT d.* FROM drives d
               JOIN projects p ON p.id = d.project_id
              WHERE p.org_id = $1
              ORDER BY d.name ASC",
        )
        .bind(org_id)
        .fetch_all(&mut *conn)
        .await
    } else {
        // Skip malformed ids rather than fail the whole search; the access gate below
        // still confines the result to reachable Drives.
        let ids: Vec<Uuid> = requested
            .iter()
            .filter_map(|id| Uuid::parse_str(id.value.trim()).ok())
            .collect();
        sqlx::query_as::<_, DriveRow>("SELECT * FROM drives WHERE id = ANY($1) ORDER BY name ASC")
            .bind(&ids)
            .fetch_all(&mut *conn)
            .await
    };
    let rows = rows.map_err(|error| {
        tracing::error!(error = %error, "search drive query failed");
        StatusCode::SERVICE_UNAVAILABLE.into_response()
    })?;

    let mut accessible = Vec::new();
    for row in rows {
        match can_access_drive(&mut conn, user_id, &row).await {
            Ok(true) => accessible.push(row),
            Ok(false) => {}
            Err(error) => {
                tracing::error!(error = %error, "search access resolution failed");
                return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
            }
        }
    }
    Ok(accessible)
}

// --- Per-source search ------------------------------------------------------

/// Searches one Drive end to end, emitting its frames into the shared sink.
///
/// Emits `SEARCHING` immediately so the UI can show this source, then opens the Drive's
/// operator (a failure here is this source's `ERROR`) and walks it. The operator is held
/// for the whole walk so an SFTP keyfile stays alive until the last entry.
async fn search_one_drive(sink: FrameSink, crypto: Crypto, row: DriveRow, query: String) {
    let drive_id = row.id.to_string();
    let drive_name = row.name.clone();

    if !sink
        .send(make_frame(
            &drive_id,
            &drive_name,
            Status::Searching,
            Vec::new(),
            "",
        ))
        .await
    {
        return;
    }

    let operator = match build_operator(&crypto, &row) {
        Ok(operator) => operator,
        Err(error) => {
            let _ = sink
                .send(make_frame(
                    &drive_id,
                    &drive_name,
                    Status::Error,
                    Vec::new(),
                    &error.to_string(),
                ))
                .await;
            return;
        }
    };

    walk_and_emit(&sink, &drive_id, &drive_name, operator.operator(), &query).await;
}

/// Recursively walks `operator`, streaming matching entries and a terminal frame.
///
/// Reads the provider's recursive lister one entry at a time (an await point that both
/// yields and lets an aborted task stop mid-walk), matching each entry's path against
/// the already-lowercased `query` and hiding the reserved `.elysium/` area. Matches
/// flush in [`MATCH_BATCH_SIZE`] batches; the walk ends with `DONE` carrying the
/// remainder. A stalled walk emits one `SLOW` frame past [`SLOW_AFTER`] and `ERROR`
/// past [`PER_SOURCE_TIMEOUT`]. Stops early and silently when the client is gone.
async fn walk_and_emit(
    sink: &FrameSink,
    drive_id: &str,
    drive_name: &str,
    operator: &Operator,
    query: &str,
) {
    let mut lister = match operator.lister_with("").recursive(true).await {
        Ok(lister) => lister,
        Err(error) => {
            let _ = sink
                .send(make_frame(
                    drive_id,
                    drive_name,
                    Status::Error,
                    Vec::new(),
                    &error.to_string(),
                ))
                .await;
            return;
        }
    };

    let mut batch: Vec<FileEntry> = Vec::new();
    let mut processed = 0usize;
    let mut slow_emitted = false;
    let started = Instant::now();
    let slow_at = started + SLOW_AFTER;
    let deadline = started + PER_SOURCE_TIMEOUT;

    loop {
        let next = tokio::select! {
            // Prefer draining the lister; the timers only fire while it is pending.
            biased;
            item = lister.next() => item,
            _ = tokio::time::sleep_until(slow_at), if !slow_emitted => {
                slow_emitted = true;
                // Carry matches so far so the SLOW signal loses nothing.
                if !sink
                    .send(make_frame(drive_id, drive_name, Status::Slow, std::mem::take(&mut batch), ""))
                    .await
                {
                    return;
                }
                continue;
            }
            _ = tokio::time::sleep_until(deadline) => {
                let _ = sink
                    .send(make_frame(
                        drive_id,
                        drive_name,
                        Status::Error,
                        std::mem::take(&mut batch),
                        "search exceeded the per-source time limit",
                    ))
                    .await;
                return;
            }
        };

        match next {
            Some(Ok(entry)) => {
                if let Some(file_entry) = match_entry(&entry, query) {
                    batch.push(file_entry);
                    if batch.len() >= MATCH_BATCH_SIZE
                        && !sink
                            .send(make_frame(
                                drive_id,
                                drive_name,
                                Status::Searching,
                                std::mem::take(&mut batch),
                                "",
                            ))
                            .await
                    {
                        return;
                    }
                }
                processed += 1;
                if processed.is_multiple_of(YIELD_EVERY) {
                    tokio::task::yield_now().await;
                }
            }
            Some(Err(error)) => {
                let _ = sink
                    .send(make_frame(
                        drive_id,
                        drive_name,
                        Status::Error,
                        std::mem::take(&mut batch),
                        &error.to_string(),
                    ))
                    .await;
                return;
            }
            None => break,
        }
    }

    let _ = sink
        .send(make_frame(drive_id, drive_name, Status::Done, batch, ""))
        .await;
}

/// Turns a walked entry into a match, or `None` when it is filtered or does not match.
///
/// Skips the listing root and the reserved `.elysium/` system area, then keeps the
/// entry when its lowercased provider-rooted path contains the (already lowercased)
/// query. Directories match too, so a folder named after the query surfaces.
fn match_entry(entry: &Entry, query: &str) -> Option<FileEntry> {
    let path = entry.path();
    if path.is_empty() {
        return None;
    }
    if files::top_segment(path) == files::RESERVED_PREFIX {
        return None;
    }
    if !path.to_lowercase().contains(query) {
        return None;
    }
    Some(files::file_entry(entry))
}

/// Builds a live operator for a Drive, opening its sealed credential.
///
/// # Errors
/// Returns an error when the stored config is malformed, the credential fails to open,
/// or the provider rejects the assembled configuration.
fn build_operator(crypto: &Crypto, row: &DriveRow) -> eyre::Result<DriveOperator> {
    let config = row.parse_config()?;
    let secret = match row.secret.as_deref() {
        Some(blob) => Some(open_secret(crypto, row.project_id, blob)?),
        None => None,
    };
    provider::operator(&config, secret.as_deref())
}

/// Builds a [`SearchFrame`] for one source with the given status and payload.
fn make_frame(
    drive_id: &str,
    drive_name: &str,
    status: Status,
    matches: Vec<FileEntry>,
    error: &str,
) -> SearchFrame {
    SearchFrame {
        drive_id: drive_id.to_string(),
        drive_name: drive_name.to_string(),
        status: status as i32,
        matches,
        error: error.to_string(),
    }
}

// --- Streaming coordinator --------------------------------------------------

/// The future one source runs to emit its frames.
type SourceFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Builds a source's future once given the shared sink; one per target Drive.
type SourceBuilder = Box<dyn FnOnce(FrameSink) -> SourceFuture + Send>;

/// The write half a source uses to emit length-delimited frames to the client.
///
/// Cloned once per source; when the client disconnects the underlying channel closes
/// and [`FrameSink::send`] returns `false`, letting a walk stop between entries.
#[derive(Clone)]
struct FrameSink {
    tx: mpsc::Sender<Bytes>,
}

impl FrameSink {
    /// Length-delimits and sends one frame; `false` means the client is gone.
    async fn send(&self, frame: SearchFrame) -> bool {
        let bytes = Bytes::from(frame.encode_length_delimited_to_vec());
        self.tx.send(bytes).await.is_ok()
    }
}

/// Runs each source concurrently under a bound and streams their frames to the client.
///
/// Creates the shared channel, spawns every source into a [`JoinSet`] owned by the
/// returned [`SearchBody`], and bounds how many walk at once with a [`Semaphore`]. The
/// original sender is dropped here, so the stream ends exactly when the last source
/// finishes. Because the body owns the `JoinSet`, dropping the body (a client
/// disconnect) aborts every in-flight source.
fn stream_frames(builders: Vec<SourceBuilder>, concurrency: usize) -> SearchBody {
    let (tx, rx) = mpsc::channel::<Bytes>(FRAME_CHANNEL_CAPACITY);
    let semaphore = Arc::new(Semaphore::new(concurrency.max(1)));
    let mut tasks = JoinSet::new();

    for builder in builders {
        let sink = FrameSink { tx: tx.clone() };
        let semaphore = Arc::clone(&semaphore);
        tasks.spawn(async move {
            // Bounded fan-out: hold a permit for the whole walk so only `concurrency`
            // sources touch their provider at once. The semaphore never closes.
            let Ok(_permit) = semaphore.acquire_owned().await else {
                return;
            };
            builder(sink).await;
        });
    }
    // Only the sources hold senders now; the receiver ends when they all finish.
    drop(tx);

    SearchBody { rx, _tasks: tasks }
}

/// The chunked response body: a stream of length-delimited frames owning its sources.
///
/// Yields each source's encoded frames in arrival order. Holding the [`JoinSet`] here is
/// the cancellation mechanism: when Axum drops the body on a client disconnect, the
/// `JoinSet` drops and aborts every still-running source walk.
struct SearchBody {
    rx: mpsc::Receiver<Bytes>,
    _tasks: JoinSet<()>,
}

impl Stream for SearchBody {
    type Item = Result<Bytes, Infallible>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().rx.poll_recv(cx).map(|frame| frame.map(Ok))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{DriveConfig, FsConfig};
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Builds a live FS operator rooted at `root` for the walk-level tests.
    fn fs_operator(root: &str) -> DriveOperator {
        provider::operator(
            &DriveConfig::Fs(FsConfig {
                root: root.to_string(),
            }),
            None,
        )
        .expect("build FS operator")
    }

    /// Seeds `operator` with a file at `path` holding a byte, creating parents.
    async fn seed(operator: &Operator, path: &str) {
        operator
            .write(path, b"x".to_vec())
            .await
            .unwrap_or_else(|error| panic!("seed {path}: {error}"));
    }

    /// Drains a receiver to exhaustion, decoding the length-delimited frame stream.
    ///
    /// Concatenates every chunk and decodes frames back with
    /// `SearchFrame::decode_length_delimited`, exercising the exact framing a client
    /// uses, then asserts the buffer was consumed whole.
    fn decode_all(mut buffer: Bytes) -> Vec<SearchFrame> {
        let mut frames = Vec::new();
        while !buffer.is_empty() {
            let frame = SearchFrame::decode_length_delimited(&mut buffer)
                .expect("each frame decodes from its length prefix");
            frames.push(frame);
        }
        frames
    }

    /// Collects a whole `SearchBody` into one concatenated byte buffer.
    async fn collect_body(mut body: SearchBody) -> Bytes {
        let mut all = Vec::new();
        while let Some(chunk) = body.next().await {
            all.extend_from_slice(&chunk.expect("body item is infallible"));
        }
        Bytes::from(all)
    }

    /// A source builder that emits `SEARCHING` then runs a real FS walk of `root`.
    fn fs_builder(
        drive_id: &'static str,
        name: &'static str,
        root: String,
        query: &'static str,
    ) -> SourceBuilder {
        Box::new(move |sink: FrameSink| {
            Box::pin(async move {
                let operator = fs_operator(&root);
                if !sink
                    .send(make_frame(
                        drive_id,
                        name,
                        Status::Searching,
                        Vec::new(),
                        "",
                    ))
                    .await
                {
                    return;
                }
                walk_and_emit(&sink, drive_id, name, operator.operator(), query).await;
            }) as SourceFuture
        })
    }

    // --- Single-source walk correctness -------------------------------------

    #[tokio::test]
    async fn walk_matches_case_insensitively_and_hides_the_reserved_area() {
        let dir = tempfile::tempdir().expect("temp dir");
        let operator = fs_operator(dir.path().to_str().expect("utf-8 temp path"));
        let operator = operator.operator();

        seed(operator, "reports/logo.png").await;
        seed(operator, "images/LOGO-final.svg").await;
        seed(operator, "notes.txt").await;
        // A matching name inside the reserved system area must never surface.
        seed(operator, ".elysium/avatars/logo.png").await;

        let (tx, mut rx) = mpsc::channel::<Bytes>(64);
        let sink = FrameSink { tx };
        // A folder named "logo" so a directory match is exercised too.
        seed(operator, "logo/keep.txt").await;
        walk_and_emit(&sink, "d1", "Primary", operator, "logo").await;
        drop(sink);

        let mut buffer = Vec::new();
        while let Some(chunk) = rx.recv().await {
            buffer.extend_from_slice(&chunk);
        }
        let frames = decode_all(Bytes::from(buffer));

        // `walk_and_emit` terminates the source with a DONE frame (the leading
        // SEARCHING is the source wrapper's job, covered by the coordinator tests).
        assert_eq!(frames.last().expect("a frame").status, Status::Done as i32);

        let matched: Vec<String> = frames
            .iter()
            .flat_map(|frame| frame.matches.iter().map(|entry| entry.path.clone()))
            .collect();
        assert!(matched.iter().any(|path| path.contains("reports/logo.png")));
        assert!(matched
            .iter()
            .any(|path| path.to_lowercase().contains("logo-final.svg")));
        assert!(
            matched.iter().any(|path| path.starts_with("logo/")),
            "a directory named after the query matches",
        );
        assert!(
            !matched.iter().any(|path| path.contains("notes.txt")),
            "a non-matching file is absent",
        );
        assert!(
            !matched.iter().any(|path| path.contains(".elysium")),
            "the reserved area is hidden from search",
        );
    }

    // --- Length-delimited framing round-trip --------------------------------

    #[tokio::test]
    async fn frames_round_trip_through_the_length_delimited_stream() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_str().expect("utf-8 temp path");
        seed(fs_operator(root).operator(), "a/logo.png").await;

        let body = stream_frames(
            vec![fs_builder("only", "Only", root.to_string(), "logo")],
            MAX_CONCURRENT_DRIVE_SEARCHES,
        );
        let frames = decode_all(collect_body(body).await);

        // The whole buffer decoded cleanly (asserted inside decode_all), and the source
        // ran SEARCHING then DONE with its match.
        assert_eq!(frames.first().unwrap().status, Status::Searching as i32);
        assert_eq!(frames.last().unwrap().status, Status::Done as i32);
        assert!(frames
            .iter()
            .flat_map(|frame| &frame.matches)
            .any(|entry| entry.path.contains("logo.png")));
    }

    // --- Incremental streaming: fast source surfaces before a slow one -------

    #[tokio::test]
    async fn a_fast_drive_completes_before_a_slow_drive() {
        let fast_dir = tempfile::tempdir().expect("temp dir");
        let slow_dir = tempfile::tempdir().expect("temp dir");
        seed(
            fs_operator(fast_dir.path().to_str().unwrap()).operator(),
            "fast/logo.png",
        )
        .await;
        seed(
            fs_operator(slow_dir.path().to_str().unwrap()).operator(),
            "slow/logo.png",
        )
        .await;

        let fast = fs_builder(
            "fast",
            "Fast",
            fast_dir.path().to_str().unwrap().to_string(),
            "logo",
        );
        // The slow source announces itself, waits, then runs the real walk. This models
        // a genuinely slow provider deterministically.
        let slow_root = slow_dir.path().to_str().unwrap().to_string();
        let slow: SourceBuilder = Box::new(move |sink: FrameSink| {
            Box::pin(async move {
                let operator = fs_operator(&slow_root);
                if !sink
                    .send(make_frame(
                        "slow",
                        "Slow",
                        Status::Searching,
                        Vec::new(),
                        "",
                    ))
                    .await
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
                walk_and_emit(&sink, "slow", "Slow", operator.operator(), "logo").await;
            }) as SourceFuture
        });

        // Read frames in arrival order.
        let mut body = stream_frames(vec![fast, slow], MAX_CONCURRENT_DRIVE_SEARCHES);
        let mut ordered: Vec<SearchFrame> = Vec::new();
        while let Some(chunk) = body.next().await {
            let bytes = chunk.expect("infallible");
            for frame in decode_all(bytes) {
                ordered.push(frame);
            }
        }

        let fast_done = ordered
            .iter()
            .position(|frame| frame.drive_id == "fast" && frame.status == Status::Done as i32)
            .expect("fast drive reports DONE");
        let slow_done = ordered
            .iter()
            .position(|frame| frame.drive_id == "slow" && frame.status == Status::Done as i32)
            .expect("slow drive reports DONE");
        assert!(
            fast_done < slow_done,
            "the fast drive finishes before the slow one",
        );

        // A fast match arrived before the slow drive finished, proving incremental
        // streaming rather than collect-all-then-send.
        let fast_match = ordered
            .iter()
            .position(|frame| frame.drive_id == "fast" && !frame.matches.is_empty())
            .expect("fast drive surfaced a match");
        assert!(
            fast_match < slow_done,
            "the fast match streamed before the slow DONE"
        );
    }

    // --- Error isolation: one source fails, the others still finish ----------

    #[tokio::test]
    async fn an_erroring_source_does_not_fail_the_others() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_str().expect("utf-8 temp path");
        seed(fs_operator(root).operator(), "a/logo.png").await;

        // A source that fails outright, emitted alongside a healthy FS source.
        let broken: SourceBuilder = Box::new(move |sink: FrameSink| {
            Box::pin(async move {
                let _ = sink
                    .send(make_frame(
                        "broken",
                        "Broken",
                        Status::Searching,
                        Vec::new(),
                        "",
                    ))
                    .await;
                let _ = sink
                    .send(make_frame(
                        "broken",
                        "Broken",
                        Status::Error,
                        Vec::new(),
                        "provider unreachable",
                    ))
                    .await;
            }) as SourceFuture
        });
        let healthy = fs_builder("healthy", "Healthy", root.to_string(), "logo");

        let body = stream_frames(vec![broken, healthy], MAX_CONCURRENT_DRIVE_SEARCHES);
        let frames = decode_all(collect_body(body).await);

        // The broken source yields an ERROR frame...
        assert!(frames
            .iter()
            .any(|frame| frame.drive_id == "broken" && frame.status == Status::Error as i32));
        // ...and the healthy source still reaches DONE with its match.
        assert!(frames
            .iter()
            .any(|frame| { frame.drive_id == "healthy" && frame.status == Status::Done as i32 }));
        assert!(frames
            .iter()
            .filter(|frame| frame.drive_id == "healthy")
            .flat_map(|frame| &frame.matches)
            .any(|entry| entry.path.contains("logo.png")));
    }

    // --- Cancellation: dropping the body aborts in-flight sources ------------

    #[tokio::test]
    async fn dropping_the_body_aborts_outstanding_sources() {
        /// Sets a flag when dropped, so a task's teardown is observable.
        struct DropFlag(Arc<AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let aborted = Arc::new(AtomicBool::new(false));
        let flag = DropFlag(Arc::clone(&aborted));

        // A source that announces itself then hangs forever. The only way its DropFlag
        // fires is the JoinSet aborting the task when the body drops.
        let hung: SourceBuilder = Box::new(move |sink: FrameSink| {
            Box::pin(async move {
                let _flag = flag;
                let _ = sink
                    .send(make_frame(
                        "hung",
                        "Hung",
                        Status::Searching,
                        Vec::new(),
                        "",
                    ))
                    .await;
                std::future::pending::<()>().await;
            }) as SourceFuture
        });

        let mut body = stream_frames(vec![hung], MAX_CONCURRENT_DRIVE_SEARCHES);
        // Pull the first frame so the source is actively running, then hang up.
        let first = body.next().await.expect("the source emits a frame");
        assert_eq!(
            decode_all(first.expect("infallible"))
                .first()
                .unwrap()
                .status,
            Status::Searching as i32,
        );
        drop(body);

        // The abort runs the source future's destructors promptly.
        let mut fired = false;
        for _ in 0..200 {
            if aborted.load(Ordering::SeqCst) {
                fired = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(fired, "dropping the body aborted the outstanding source");
    }
}
