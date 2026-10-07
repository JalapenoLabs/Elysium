// Copyright © 2026 Jalapeno Labs

//! A local fake of a satellite, for tests that drive the real SDK.
//!
//! It holds threads, each with a state and a workspace, and answers every route Elysium's
//! flows call, the way a satellite does:
//!
//! - `GET /v1/version`, and `GET`, `POST`, and `DELETE` on threads, in protobuf. A thread it
//!   does not hold answers `THREAD_NOT_FOUND`.
//! - Workspace files: the file's bytes for a read, a `WorkspaceFileWritten` for a write, and
//!   `WORKSPACE_FILE_NOT_FOUND` for a file that is not there.
//! - The artifact listing, which like a satellite's is every file under `artifacts/` with its
//!   size and SHA-256.
//! - Turns, which are queued and answered with the turn, or refused when a test asks.
//! - The harness session: an export answers the archive a test set, with the harness and
//!   session id in its headers, or `HARNESS_SESSION_NOT_FOUND`; an import keeps the archive.
//! - The relay socket, refused with `RELAY_NOT_DECLARED`, so the relay a new session starts
//!   stops at once rather than retrying for the rest of the test.
//!
//! A test reads what the satellite was asked through [`FakeSatellite`] (the threads created,
//! the turns started, the sessions imported) and steers what it answers: a thread's state, a
//! refusal for turns, the session to export, and a [`Hold`] that stops one request until the
//! test releases it, which makes a race between two requests deterministic.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use arsox_sdk::client::{Satellite as SatelliteClient, ThreadHandle};
use arsox_sdk::proto::artifact::v1::{Artifact, ListArtifactsResponse, WorkspaceFileWritten};
use arsox_sdk::proto::error::v1::{Error as ContractError, ErrorCode};
use arsox_sdk::proto::harness::v1::{Harness, ImportHarnessSessionResponse};
use arsox_sdk::proto::satellite::v1::GetVersionResponse;
use arsox_sdk::proto::thread::v1::{
    CreateThreadResponse, DestroyThreadResponse, GetThreadResponse, Thread, ThreadState,
};
use arsox_sdk::proto::turn::v1::{StartTurnRequest, StartTurnResponse, Turn, TurnStatus};
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures_util::StreamExt as _;
use prost::Message as _;
use sha2::{Digest as _, Sha256};
use tokio::sync::Notify;
use uuid::Uuid;

/// The thread every fake satellite starts with, idle.
pub const THREAD_ID: &str = "thread-1";

/// The response headers a satellite names an exported session's harness and id in. The SDK
/// keeps its own copies private.
const HARNESS_HEADER: &str = "arsox-harness";
const SESSION_ID_HEADER: &str = "arsox-harness-session-id";

/// A workspace file as the fake holds it, with the headers it was written with.
#[derive(Debug, Clone)]
pub struct WorkspaceFile {
    pub headers: HeaderMap,
    pub bytes: Vec<u8>,
}

/// One thread's workspace files by path, such as `reports/today.txt`.
pub type Workspace = Arc<Mutex<BTreeMap<String, WorkspaceFile>>>;

/// A harness session as the fake exports it.
#[derive(Debug, Clone)]
pub struct SessionArchive {
    pub harness: Harness,
    pub harness_session_id: String,
    pub bytes: Vec<u8>,
}

/// An archive handed to a thread with `import_session`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedSession {
    pub thread_id: String,
    pub archive: Vec<u8>,
}

/// A request a test can hold until it releases it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoldPoint {
    /// `POST /v1/threads`.
    ThreadCreate,
    /// `GET /v1/threads/{id}` for this thread.
    ThreadRead(String),
}

/// The next request at a [`HoldPoint`], held. Only one request is held: those after it pass.
#[derive(Debug, Default)]
pub struct Hold {
    arrived: Notify,
    released: Notify,
}

impl Hold {
    /// Waits until the held request reaches the fake.
    pub async fn arrived(&self) {
        self.arrived.notified().await;
    }

    /// Lets the held request carry on.
    pub fn release(&self) {
        self.released.notify_one();
    }
}

#[derive(Debug)]
struct FakeThread {
    state: ThreadState,
    workspace: Workspace,
}

impl FakeThread {
    fn idle() -> Self {
        Self {
            state: ThreadState::Idle,
            workspace: Workspace::default(),
        }
    }
}

/// Everything the fake holds and was asked. Never locked across an await.
#[derive(Debug, Default)]
struct Inner {
    threads: BTreeMap<String, FakeThread>,
    /// Threads opened with `POST /v1/threads`, in order.
    created: Vec<String>,
    turns: Vec<StartTurnRequest>,
    turn_refusal: Option<ErrorCode>,
    export: Option<SessionArchive>,
    imported: Vec<ImportedSession>,
    hold: Option<(HoldPoint, Arc<Hold>)>,
}

/// A running fake satellite, and the test's view of it. Clones share it.
#[derive(Debug, Clone)]
pub struct FakeSatellite {
    url: String,
    inner: Arc<Mutex<Inner>>,
}

impl FakeSatellite {
    /// Starts a fake on a free local port, holding [`THREAD_ID`], idle.
    pub async fn start() -> Self {
        let inner = Inner {
            threads: BTreeMap::from([(THREAD_ID.to_owned(), FakeThread::idle())]),
            ..Inner::default()
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        let satellite = Self {
            url: format!("http://{address}"),
            inner: Arc::new(Mutex::new(inner)),
        };

        let router = Router::new()
            .route("/v1/version", get(version))
            .route("/v1/threads", post(create_thread))
            .route("/v1/threads/{id}", get(read_thread).delete(destroy_thread))
            .route("/v1/threads/{id}/turns", post(start_turn))
            .route(
                "/v1/threads/{id}/session",
                get(export_session).put(import_session),
            )
            .route("/v1/threads/{id}/relay", get(refuse_relay))
            .route("/v1/threads/{id}/artifacts", get(list_artifacts))
            .route(
                "/v1/threads/{id}/files/{*path}",
                get(read_file).put(write_file),
            )
            .with_state(satellite.clone());
        tokio::spawn(async move { axum::serve(listener, router).await });
        satellite
    }

    /// The base URL a satellite row points at to reach this fake.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Connects through the SDK and attaches to one of the fake's threads.
    pub async fn attach(&self, thread_id: &str) -> ThreadHandle {
        let client = SatelliteClient::connect(self.url.clone(), "satellite-secret")
            .await
            .expect("connect");
        client.threads().attach(thread_id).await.expect("attach")
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("the fake satellite's lock")
    }

    /// A thread's workspace, which the test may read and write directly.
    ///
    /// # Panics
    /// Panics when the fake holds no such thread.
    pub fn workspace(&self, thread_id: &str) -> Workspace {
        let inner = self.lock();
        let thread = inner.threads.get(thread_id).expect("the thread exists");
        Arc::clone(&thread.workspace)
    }

    /// Moves a thread to `state`, such as [`ThreadState::Expired`] for one that has ended.
    ///
    /// # Panics
    /// Panics when the fake holds no such thread.
    pub fn set_thread_state(&self, thread_id: &str, state: ThreadState) {
        let mut inner = self.lock();
        inner
            .threads
            .get_mut(thread_id)
            .expect("the thread exists")
            .state = state;
    }

    /// The threads opened through the SDK, oldest first.
    pub fn created_threads(&self) -> Vec<String> {
        self.lock().created.clone()
    }

    /// Every turn the fake accepted, oldest first, as the SDK sent it.
    pub fn turns(&self) -> Vec<StartTurnRequest> {
        self.lock().turns.clone()
    }

    /// Every archive imported into a thread, oldest first.
    pub fn imported_sessions(&self) -> Vec<ImportedSession> {
        self.lock().imported.clone()
    }

    /// Refuses every turn from now on with `code`.
    pub fn refuse_turns(&self, code: ErrorCode) {
        self.lock().turn_refusal = Some(code);
    }

    /// Answers every session export with `archive`. Until this is called, an export answers
    /// `HARNESS_SESSION_NOT_FOUND`, as for a thread that has not run a turn.
    pub fn set_session_export(&self, archive: SessionArchive) {
        self.lock().export = Some(archive);
    }

    /// Holds the next request at `point` until the returned [`Hold`] is released.
    pub fn hold(&self, point: HoldPoint) -> Arc<Hold> {
        let hold = Arc::new(Hold::default());
        self.lock().hold = Some((point, Arc::clone(&hold)));
        hold
    }

    /// Waits for the test's release when this request is the one it holds.
    async fn pass(&self, point: &HoldPoint) {
        let hold = {
            let mut inner = self.lock();
            let is_held = inner.hold.as_ref().is_some_and(|(held, _)| held == point);
            if is_held {
                inner.hold.take().map(|(_point, hold)| hold)
            } else {
                None
            }
        };
        if let Some(hold) = hold {
            hold.arrived.notify_one();
            hold.released.notified().await;
        }
    }
}

fn protobuf(status: StatusCode, message: &impl prost::Message) -> Response {
    let headers = [(header::CONTENT_TYPE, "application/protobuf")];
    (status, headers, message.encode_to_vec()).into_response()
}

fn refusal(status: StatusCode, code: ErrorCode, message: String) -> Response {
    let error = ContractError {
        code: code.into(),
        message,
        ..ContractError::default()
    };
    protobuf(status, &error)
}

fn thread_not_found(thread_id: &str) -> Response {
    refusal(
        StatusCode::NOT_FOUND,
        ErrorCode::ThreadNotFound,
        format!("no thread {thread_id}"),
    )
}

/// The thread as the satellite reports it.
fn thread_view(thread_id: String, state: ThreadState) -> Thread {
    Thread {
        thread_id,
        state: state.into(),
        ..Thread::default()
    }
}

/// Reads a request body whole, as the satellite does before it answers.
async fn receive(body: Body) -> Option<Vec<u8>> {
    let mut chunks = body.into_data_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = chunks.next().await {
        bytes.extend_from_slice(&chunk.ok()?);
    }
    Some(bytes)
}

async fn version() -> Response {
    let answer = GetVersionResponse {
        satellite_version: "test".to_owned(),
        proto_major: 1,
        proto_minor: 0,
    };
    protobuf(StatusCode::OK, &answer)
}

async fn create_thread(State(satellite): State<FakeSatellite>) -> Response {
    satellite.pass(&HoldPoint::ThreadCreate).await;
    // A satellite names its threads with UUIDv7s, so two never collide.
    let thread_id = Uuid::now_v7().to_string();
    let mut inner = satellite.lock();
    inner.threads.insert(thread_id.clone(), FakeThread::idle());
    inner.created.push(thread_id.clone());
    drop(inner);
    let answer = CreateThreadResponse {
        thread: Some(thread_view(thread_id, ThreadState::Idle)),
        deduplicated: false,
    };
    protobuf(StatusCode::OK, &answer)
}

async fn read_thread(
    State(satellite): State<FakeSatellite>,
    Path(thread_id): Path<String>,
) -> Response {
    satellite
        .pass(&HoldPoint::ThreadRead(thread_id.clone()))
        .await;
    let state = satellite
        .lock()
        .threads
        .get(&thread_id)
        .map(|thread| thread.state);
    let Some(state) = state else {
        return thread_not_found(&thread_id);
    };
    let answer = GetThreadResponse {
        thread: Some(thread_view(thread_id, state)),
    };
    protobuf(StatusCode::OK, &answer)
}

async fn destroy_thread(
    State(satellite): State<FakeSatellite>,
    Path(thread_id): Path<String>,
) -> Response {
    let destroyed = match satellite.lock().threads.get_mut(&thread_id) {
        Some(thread) => {
            thread.state = ThreadState::Destroyed;
            true
        }
        None => false,
    };
    if !destroyed {
        return thread_not_found(&thread_id);
    }
    let answer = DestroyThreadResponse {
        thread: Some(thread_view(thread_id, ThreadState::Destroyed)),
    };
    protobuf(StatusCode::OK, &answer)
}

async fn start_turn(
    State(satellite): State<FakeSatellite>,
    Path(thread_id): Path<String>,
    body: Bytes,
) -> Response {
    let Ok(request) = StartTurnRequest::decode(body) else {
        return refusal(
            StatusCode::BAD_REQUEST,
            ErrorCode::RequestBodyMalformed,
            "not a StartTurnRequest".to_owned(),
        );
    };
    let mut inner = satellite.lock();
    if !inner.threads.contains_key(&thread_id) {
        return thread_not_found(&thread_id);
    }
    if let Some(code) = inner.turn_refusal {
        return refusal(
            StatusCode::CONFLICT,
            code,
            "the satellite refused the turn".to_owned(),
        );
    }

    let turn = Turn {
        turn_id: Uuid::now_v7().to_string(),
        thread_id,
        status: TurnStatus::Queued.into(),
        prompt: request.prompt.clone(),
        ..Turn::default()
    };
    inner.turns.push(request);
    drop(inner);
    protobuf(StatusCode::OK, &StartTurnResponse { turn: Some(turn) })
}

async fn export_session(State(satellite): State<FakeSatellite>) -> Response {
    let Some(archive) = satellite.lock().export.clone() else {
        return refusal(
            StatusCode::NOT_FOUND,
            ErrorCode::HarnessSessionNotFound,
            "the thread has no harness session yet".to_owned(),
        );
    };
    // axum sets the Content-Length of a whole body, which the SDK requires of an export.
    let headers = [
        (header::CONTENT_TYPE, "application/x-tar".to_owned()),
        (
            header::HeaderName::from_static(HARNESS_HEADER),
            archive.harness.as_str_name().to_owned(),
        ),
        (
            header::HeaderName::from_static(SESSION_ID_HEADER),
            archive.harness_session_id,
        ),
    ];
    (headers, archive.bytes).into_response()
}

async fn import_session(
    State(satellite): State<FakeSatellite>,
    Path(thread_id): Path<String>,
    body: Body,
) -> Response {
    let Some(archive) = receive(body).await else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    satellite
        .lock()
        .imported
        .push(ImportedSession { thread_id, archive });
    protobuf(StatusCode::OK, &ImportHarnessSessionResponse::default())
}

async fn refuse_relay() -> Response {
    refusal(
        StatusCode::CONFLICT,
        ErrorCode::RelayNotDeclared,
        "the thread declares no relayed servers".to_owned(),
    )
}

async fn read_file(
    State(satellite): State<FakeSatellite>,
    Path((thread_id, path)): Path<(String, String)>,
) -> Response {
    let file = {
        let inner = satellite.lock();
        let Some(thread) = inner.threads.get(&thread_id) else {
            return thread_not_found(&thread_id);
        };
        thread.workspace.lock().expect("lock").get(&path).cloned()
    };
    let Some(file) = file else {
        return refusal(
            StatusCode::NOT_FOUND,
            ErrorCode::WorkspaceFileNotFound,
            format!("nothing is at {path}"),
        );
    };
    let headers = [(header::CONTENT_TYPE, "text/csv")];
    (headers, file.bytes).into_response()
}

async fn write_file(
    State(satellite): State<FakeSatellite>,
    Path((thread_id, path)): Path<(String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let Some(bytes) = receive(body).await else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let written = WorkspaceFileWritten {
        path: path.clone(),
        size_bytes: bytes.len() as u64,
        ..WorkspaceFileWritten::default()
    };
    let inner = satellite.lock();
    let Some(thread) = inner.threads.get(&thread_id) else {
        return thread_not_found(&thread_id);
    };
    thread
        .workspace
        .lock()
        .expect("lock")
        .insert(path, WorkspaceFile { headers, bytes });
    drop(inner);
    protobuf(StatusCode::OK, &written)
}

async fn list_artifacts(
    State(satellite): State<FakeSatellite>,
    Path(thread_id): Path<String>,
) -> Response {
    let inner = satellite.lock();
    let Some(thread) = inner.threads.get(&thread_id) else {
        return thread_not_found(&thread_id);
    };
    let artifacts = thread
        .workspace
        .lock()
        .expect("lock")
        .iter()
        .filter_map(|(path, file)| {
            let relative = path.strip_prefix("artifacts/")?;
            Some(Artifact {
                name: relative.rsplit('/').next().unwrap_or(relative).to_owned(),
                path: relative.to_owned(),
                size_bytes: file.bytes.len() as u64,
                sha256: hex::encode(Sha256::digest(&file.bytes)),
                ..Artifact::default()
            })
        })
        .collect();
    drop(inner);
    let answer = ListArtifactsResponse {
        artifacts,
        page: None,
    };
    protobuf(StatusCode::OK, &answer)
}

/// Starts a fake satellite and attaches to its one thread, for tests that need a workspace
/// and nothing else.
pub async fn fake_workspace() -> (ThreadHandle, Workspace) {
    let satellite = FakeSatellite::start().await;
    let handle = satellite.attach(THREAD_ID).await;
    (handle, satellite.workspace(THREAD_ID))
}
