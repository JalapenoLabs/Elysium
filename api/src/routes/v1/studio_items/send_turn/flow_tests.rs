// Copyright © 2026 Jalapeno Labs

//! Sending a Studio prompt end to end: the real router with a signed-in request, the real SDK
//! against the fleet's fake satellite, [`Storage`](crate::storage::Storage) against the storage
//! module's fake Bunny zone, Postgres, and Redis.
//!
//! The first half sends to an item whose thread is live, plain and drawn (`docs/studio.md`,
//! Feedback). The second continues an item whose thread has ended, by importing its harness
//! session or by opening with a brief, and races two prompts at it (Continuing).

use arsox_sdk::proto::error::v1::ErrorCode;
use arsox_sdk::proto::thread::v1::ThreadState;
use axum::body::Body;
use axum::http::{HeaderMap, Request, header};
use diesel_async::AsyncPgConnection;
use secrecy::SecretString;
use sha2::{Digest as _, Sha256};
use tower::ServiceExt as _;

use super::*;
use crate::fleet::fake_satellite::{FakeSatellite, HoldPoint, ImportedSession, THREAD_ID};
use crate::models::coding_session::{self, CodingSession, SessionContinuation};
use crate::models::llm::{self, LlmType, NewLlm};
use crate::models::satellite::{self, NewSatellite};
use crate::models::session_transcript::{self, HarnessFamily};
use crate::models::studio_asset::{NewStudioAsset, StudioAsset, StudioAssetKind};
use crate::storage::tests::{Files, StoredFile, fake_storage};
use crate::test_support::{
    TEST_PERSON_ID, app_state_with_storage, migrated_database, multipart_form, signed_in_cookie,
    studio_item_with_session,
};

/// The origin `app_state` serves, which an unsafe request must come from.
const ORIGIN: &str = "http://localhost:4000";

/// What every brief opens with.
const BRIEF_OPENING: &str = "This item continues work from an earlier conversation";

/// An item with one session on a fake satellite, and a signed-in person to prompt it.
struct Studio {
    state: AppState,
    connection: AsyncPgConnection,
    cookie: String,
    satellite: FakeSatellite,
    satellite_id: Uuid,
    item: StudioItem,
    /// The item's first session, on the fake's [`THREAD_ID`].
    session: CodingSession,
    /// The fake zone's files by key.
    stored: Files,
}

impl Studio {
    async fn start() -> Self {
        let (url, mut connection) = migrated_database().await;
        let (storage, stored) = fake_storage().await;
        let state = app_state_with_storage(&url, storage).await;
        let cookie = signed_in_cookie(&state).await;
        let satellite = FakeSatellite::start().await;
        let fixture =
            studio_item_with_session(&mut connection, &state.cipher, satellite.url()).await;
        Self {
            state,
            connection,
            cookie,
            satellite,
            satellite_id: fixture.satellite_id,
            item: fixture.item,
            session: fixture.session,
            stored,
        }
    }

    /// A prompt to the item, ready to send.
    fn prompt(&self, parts: &[(&str, &[u8])]) -> Request<Body> {
        let (content_type, body) = multipart_form(parts);
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/studio-items/{}/turns", self.item.id))
            .header(header::COOKIE, &self.cookie)
            .header(header::ORIGIN, ORIGIN)
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(body))
            .expect("a request builds")
    }

    async fn send(&self, parts: &[(&str, &[u8])]) -> (StatusCode, Value) {
        answer(self.state.clone(), self.prompt(parts)).await
    }

    /// Ends the item's first thread, as its idle TTL would.
    fn end_thread(&self) {
        self.satellite
            .set_thread_state(THREAD_ID, ThreadState::Expired);
    }

    /// Keeps a version of one of the item's files, as the session watcher does when the agent
    /// announces it: the bytes in the fake zone, named by content, and a row.
    async fn keep_file(&mut self, artifact_path: &str, bytes: &[u8]) -> StudioAsset {
        let sha256 = hex::encode(Sha256::digest(bytes));
        let storage_path = studio_asset::storage_path(self.item.id, &sha256, artifact_path);
        self.stored.lock().expect("lock").insert(
            storage_path.clone(),
            StoredFile {
                headers: HeaderMap::new(),
                bytes: bytes.to_vec(),
            },
        );
        let kind = studio_asset::kind_of(artifact_path);
        let content_type = match kind {
            StudioAssetKind::Image => "image/png",
            StudioAssetKind::Model => "model/gltf-binary",
            StudioAssetKind::File => "application/octet-stream",
        };
        studio_asset::create(
            &mut self.connection,
            &NewStudioAsset {
                id: Uuid::now_v7(),
                studio_item_id: self.item.id,
                session_id: Some(self.session.id),
                kind,
                artifact_path: artifact_path.to_owned(),
                content_type: Some(content_type.to_owned()),
                size_bytes: i64::try_from(bytes.len()).expect("a small file"),
                sha256,
                storage_path,
            },
        )
        .await
        .expect("the file is recorded")
        .expect("a new version")
    }

    /// Records a drawn prompt the item had before its thread ended.
    async fn record_feedback(&mut self, prompt: &str) {
        let id = Uuid::now_v7();
        let annotated_storage_path = format!(
            "{}/feedback/{id}/annotated.png",
            studio_asset::item_directory(self.item.id)
        );
        let feedback = StudioFeedback {
            id,
            studio_item_id: self.item.id,
            session_id: Some(self.session.id),
            turn_id: Some("turn-0".to_owned()),
            prompt: prompt.to_owned(),
            source_asset_id: None,
            camera_orbit: None,
            annotated_storage_path,
            annotated_size_bytes: 1,
            capture_storage_path: None,
            capture_size_bytes: None,
            created_by: TEST_PERSON_ID,
            created_at: Utc::now(),
        };
        studio_feedback::create(&mut self.connection, &feedback)
            .await
            .expect("the feedback is recorded");
    }

    /// Gives new threads a Claude credential, so they run the Claude harness.
    async fn use_claude(&mut self) {
        llm::create(
            &mut self.connection,
            &self.state.cipher,
            &NewLlm {
                name: "Claude".to_owned(),
                description: String::new(),
                type_: LlmType::ClaudeApiToken,
                secret_token: SecretString::from("sk-ant-test"),
                priority: 0,
                is_active: true,
                expires_at: None,
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("the credential is created");
    }

    /// Keeps a harness session for the item's first thread, as a backup after a turn does.
    async fn save_transcript(&mut self, family: HarnessFamily, archive: &[u8]) {
        session_transcript::store(
            &mut self.connection,
            &self.state.cipher,
            self.session.id,
            family,
            "harness-session-1",
            archive,
        )
        .await
        .expect("the transcript is kept");
    }

    /// The item's sessions, oldest first.
    async fn sessions(&mut self) -> Vec<CodingSession> {
        coding_session::list_for_studio_items(&mut self.connection, &[self.item.id])
            .await
            .expect("sessions")
    }

    /// The one thread opened since the test began.
    fn new_thread(&self) -> String {
        let created = self.satellite.created_threads();
        assert_eq!(created.len(), 1, "exactly one thread opened: {created:?}");
        created[0].clone()
    }

    /// The bytes at `path` in a thread's workspace.
    fn workspace_file(&self, thread_id: &str, path: &str) -> Option<Vec<u8>> {
        let workspace = self.satellite.workspace(thread_id);
        let files = workspace.lock().expect("lock");
        files.get(path).map(|file| file.bytes.clone())
    }

    /// The bytes kept at `key` in the fake zone.
    fn stored_file(&self, key: &str) -> Option<Vec<u8>> {
        let stored = self.stored.lock().expect("lock");
        stored.get(key).map(|file| file.bytes.clone())
    }
}

/// Sends a request through the whole router, as the browser's would arrive.
async fn answer(state: AppState, request: Request<Body>) -> (StatusCode, Value) {
    let response = crate::routes::router(&state)
        .with_state(state.clone())
        .oneshot(request)
        .await
        .expect("the router answers");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("the body reads");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// A PNG as far as the route checks: the signature, then anything.
fn png(content: &str) -> Vec<u8> {
    let mut bytes = PNG_SIGNATURE.to_vec();
    bytes.extend_from_slice(content.as_bytes());
    bytes
}

fn attachment(path: &str, bytes: &[u8]) -> TurnAttachment {
    TurnAttachment {
        path: path.to_owned(),
        content_type: Some("image/png".to_owned()),
        size_bytes: bytes.len() as u64,
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_plain_prompt_to_a_live_thread_runs_there_as_written() {
    let mut studio = Studio::start().await;

    let (status, body) = studio.send(&[("prompt", b"Make it yellower")]).await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["continued"], false);
    assert_eq!(body["session"]["id"], studio.session.id);
    assert!(body["feedback"].is_null(), "{body}");
    assert!(studio.satellite.created_threads().is_empty());
    assert_eq!(studio.sessions().await.len(), 1, "no new session");

    let turns = studio.satellite.turns();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].thread_id, THREAD_ID);
    assert_eq!(turns[0].prompt, "Make it yellower");
    assert!(
        turns[0].attachments.is_empty(),
        "a plain prompt attaches nothing"
    );
    assert_eq!(body["turn"]["prompt"], "Make it yellower");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_drawn_prompt_keeps_both_images_and_attaches_the_drawing() {
    let mut studio = Studio::start().await;
    let annotated = png("the drawing over the view");
    let capture = png("the clean view");

    let (status, body) = studio
        .send(&[
            ("prompt", b"Shorten the stem"),
            ("cameraOrbit", b"30deg 75deg 2m"),
            ("annotated", &annotated),
            ("capture", &capture),
        ])
        .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["continued"], false);
    let feedback_id: Uuid = body["feedback"]["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .expect("the response carries the feedback");
    let turn_id = body["turn"]["turnId"].as_str().expect("a turn id");
    assert_eq!(body["feedback"]["turnId"], turn_id);
    assert_eq!(body["feedback"]["createdBy"], TEST_PERSON_ID.to_string());

    let directory = format!("studio/{}/feedback/{feedback_id}", studio.item.id);
    assert_eq!(
        studio.stored_file(&format!("{directory}/annotated.png")),
        Some(annotated.clone())
    );
    assert_eq!(
        studio.stored_file(&format!("{directory}/capture.png")),
        Some(capture)
    );

    let workspace_path = format!("feedback/{feedback_id}/annotated.png");
    assert_eq!(
        studio.workspace_file(THREAD_ID, &workspace_path),
        Some(annotated.clone()),
        "the drawing is in the workspace for the harness to read"
    );
    let turns = studio.satellite.turns();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].prompt, "Shorten the stem");
    assert_eq!(
        turns[0].attachments,
        [attachment(&workspace_path, &annotated)]
    );

    let row = studio_feedback::find_for_item(&mut studio.connection, studio.item.id, feedback_id)
        .await
        .expect("the drawn prompt is recorded");
    assert_eq!(
        row.turn_id.as_deref(),
        Some(turn_id),
        "named after its turn"
    );
    assert_eq!(row.created_by, TEST_PERSON_ID);
    assert_eq!(row.session_id, Some(studio.session.id));
    assert_eq!(row.prompt, "Shorten the stem");
    assert_eq!(row.camera_orbit.as_deref(), Some("30deg 75deg 2m"));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_refused_turn_takes_its_drawing_with_it() {
    let mut studio = Studio::start().await;
    studio.satellite.refuse_turns(ErrorCode::TurnQueueFull);

    let (status, body) = studio
        .send(&[
            ("prompt", b"Shorten the stem"),
            ("annotated", &png("the drawing")),
            ("capture", &png("the clean view")),
        ])
        .await;

    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    let rows = studio_feedback::list_for_item(&mut studio.connection, studio.item.id)
        .await
        .expect("feedback");
    assert!(rows.is_empty(), "the row is removed: {rows:?}");
    let feedback_directory = format!("studio/{}/feedback/", studio.item.id);
    let stored = studio.stored.lock().expect("lock");
    let left: Vec<&String> = stored
        .keys()
        .filter(|key| key.starts_with(&feedback_directory))
        .collect();
    assert!(left.is_empty(), "both images are removed: {left:?}");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn an_ended_thread_continues_with_a_brief_when_no_session_was_saved() {
    let mut studio = Studio::start().await;
    studio
        .keep_file("banana-hero.png", b"the first render")
        .await;
    studio
        .keep_file("banana-hero.png", b"the second render")
        .await;
    studio.keep_file("banana.glb", b"glTF binary").await;
    studio.record_feedback("Add a stem").await;
    studio.end_thread();

    let (status, body) = studio.send(&[("prompt", b"Make it yellower")]).await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["continued"], true);
    let thread_id = studio.new_thread();
    assert_eq!(body["session"]["threadId"], thread_id.as_str());
    assert_eq!(
        body["session"]["satelliteId"],
        studio.satellite_id.to_string(),
        "it continues on the previous satellite"
    );
    assert_eq!(body["session"]["continuation"], "brief");

    let sessions = studio.sessions().await;
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[1].continuation, Some(SessionContinuation::Brief));

    assert_eq!(
        studio.workspace_file(&thread_id, "artifacts/banana-hero.png"),
        Some(b"the second render".to_vec()),
        "the newest version of each file is copied back"
    );
    assert_eq!(
        studio.workspace_file(&thread_id, "artifacts/banana.glb"),
        Some(b"glTF binary".to_vec())
    );
    assert!(studio.satellite.imported_sessions().is_empty());

    let turns = studio.satellite.turns();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].thread_id, thread_id);
    let prompt = &turns[0].prompt;
    assert!(prompt.starts_with(BRIEF_OPENING), "{prompt}");
    assert!(
        prompt.contains("Model a banana"),
        "the first prompt: {prompt}"
    );
    assert!(prompt.contains("- Add a stem"), "the feedback: {prompt}");
    assert!(
        prompt.ends_with("Now:\nMake it yellower"),
        "the person's prompt closes it: {prompt}"
    );
    assert_eq!(
        turns[0].attachments,
        [attachment(
            "artifacts/banana-hero.png",
            b"the second render"
        )],
        "the thumbnail is attached"
    );
}

/// Without a pinned image or a hero render, the tile shows the newest image, so the brief
/// attaches that one too.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_brief_attaches_the_image_the_tile_shows() {
    let mut studio = Studio::start().await;
    // The newest image sorts last by path, where the item's files are read in.
    studio.keep_file("front.png", b"the front view").await;
    let newest = studio.keep_file("side.png", b"the side view").await;
    let assets = studio_asset::list_for_items(&mut studio.connection, &[studio.item.id])
        .await
        .expect("assets");
    let tile = crate::studio::thumbnail::thumbnail_of(None, &assets.iter().collect::<Vec<_>>());
    assert_eq!(tile, Some(newest.id), "the tile shows the newest image");
    studio.end_thread();

    let (status, body) = studio.send(&[("prompt", b"Make it yellower")]).await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    let turns = studio.satellite.turns();
    assert_eq!(
        turns[0].attachments,
        [attachment("artifacts/side.png", b"the side view")]
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_continue_whose_turn_never_starts_is_undone_so_the_next_prompt_continues_afresh() {
    let mut studio = Studio::start().await;
    studio.keep_file("banana-hero.png", b"the render").await;
    studio.end_thread();
    studio.satellite.refuse_turns(ErrorCode::TurnQueueFull);

    let (status, body) = studio.send(&[("prompt", b"Make it yellower")]).await;

    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(
        studio.sessions().await.len(),
        1,
        "the session the refused turn was to run in is discarded"
    );

    studio.satellite.accept_turns();
    let (status, body) = studio.send(&[("prompt", b"Make it yellower")]).await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        body["continued"], true,
        "the retry continues the item itself"
    );
    assert_eq!(body["session"]["continuation"], "brief");
    let turns = studio.satellite.turns();
    let prompt = &turns.last().expect("the retried turn").prompt;
    assert!(
        prompt.starts_with(BRIEF_OPENING),
        "the retry carries the brief: {prompt}"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn an_ended_thread_resumes_its_saved_session_when_the_harness_matches() {
    let mut studio = Studio::start().await;
    studio.use_claude().await;
    studio.keep_file("banana-hero.png", b"the render").await;
    let archive = b"a Claude transcript, as one tar archive";
    studio.save_transcript(HarnessFamily::Claude, archive).await;
    studio.end_thread();

    let (status, body) = studio.send(&[("prompt", b"Make it yellower")]).await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["continued"], true);
    assert_eq!(body["session"]["continuation"], "imported");
    let thread_id = studio.new_thread();
    assert_eq!(
        studio.satellite.imported_sessions(),
        [ImportedSession {
            thread_id: thread_id.clone(),
            archive: archive.to_vec(),
        }]
    );
    assert_eq!(
        studio.workspace_file(&thread_id, "artifacts/banana-hero.png"),
        Some(b"the render".to_vec()),
        "files are copied back whichever way the item continues"
    );

    let turns = studio.satellite.turns();
    assert_eq!(turns.len(), 1);
    assert_eq!(
        turns[0].prompt, "Make it yellower",
        "the harness has the context; the prompt is the person's alone"
    );
    assert!(turns[0].attachments.is_empty());
    let sessions = studio.sessions().await;
    assert_eq!(
        sessions[1].continuation,
        Some(SessionContinuation::Imported)
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_saved_session_of_the_other_harness_continues_with_a_brief() {
    let mut studio = Studio::start().await;
    studio.use_claude().await;
    studio
        .save_transcript(HarnessFamily::Codex, b"a Codex rollout")
        .await;
    studio.end_thread();

    let (status, body) = studio.send(&[("prompt", b"Make it yellower")]).await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["session"]["continuation"], "brief");
    assert!(
        studio.satellite.imported_sessions().is_empty(),
        "Claude cannot resume a Codex session"
    );
    let turns = studio.satellite.turns();
    assert!(turns[0].prompt.starts_with(BRIEF_OPENING), "{turns:?}");
    assert!(turns[0].prompt.ends_with("Now:\nMake it yellower"));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn an_item_whose_satellite_was_deleted_continues_only_where_it_is_told() {
    let mut studio = Studio::start().await;
    satellite::delete(&mut studio.connection, studio.satellite_id)
        .await
        .expect("the satellite is deleted");

    let (status, body) = studio.send(&[("prompt", b"Make it yellower")]).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("choose a satellite")),
        "{body}"
    );
    assert!(studio.satellite.created_threads().is_empty());

    let chosen = satellite::create(
        &mut studio.connection,
        &studio.state.cipher,
        &NewSatellite {
            created_by: TEST_PERSON_ID,
            name: "nova".to_owned(),
            description: String::new(),
            url: studio.satellite.url().to_owned(),
            secret: SecretString::from("satellite-secret"),
            is_active: true,
        },
    )
    .await
    .expect("a satellite to continue on")
    .id;
    let chosen_text = chosen.to_string();

    let (status, body) = studio
        .send(&[
            ("prompt", b"Make it yellower"),
            ("satelliteId", chosen_text.as_bytes()),
        ])
        .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["continued"], true);
    assert_eq!(body["session"]["satelliteId"], chosen_text);
    assert_eq!(body["session"]["threadId"], studio.new_thread().as_str());
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_prompt_sent_while_the_item_is_being_continued_is_refused() {
    let mut studio = Studio::start().await;
    studio.end_thread();
    let creating = studio.satellite.hold(HoldPoint::ThreadCreate);

    // The first prompt takes the item's lock and stops while its thread is being opened.
    let first = tokio::spawn(answer(
        studio.state.clone(),
        studio.prompt(&[("prompt", b"Make it yellower")]),
    ));
    creating.arrived().await;

    let (status, body) = studio.send(&[("prompt", b"And shorter")]).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("already being continued")),
        "{body}"
    );

    creating.release();
    let (status, body) = first.await.expect("the first prompt finishes");
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["continued"], true);
    studio.new_thread();
    assert_eq!(studio.sessions().await.len(), 2, "one continued session");
}

/// A prompt that saw the item's thread ended, but takes the lock only after another prompt
/// continued the item and let it go, runs in the thread that one opened.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_prompt_that_raced_a_continue_runs_in_the_thread_it_opened() {
    let mut studio = Studio::start().await;
    studio.end_thread();
    let reading = studio
        .satellite
        .hold(HoldPoint::ThreadRead(THREAD_ID.to_owned()));

    // The late prompt has read the item's latest session and stops while it asks about its
    // thread.
    let late = tokio::spawn(answer(
        studio.state.clone(),
        studio.prompt(&[("prompt", b"And shorter")]),
    ));
    reading.arrived().await;

    let (status, first) = studio.send(&[("prompt", b"Make it yellower")]).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    assert_eq!(first["continued"], true);

    reading.release();
    let (status, late) = late.await.expect("the late prompt finishes");
    assert_eq!(status, StatusCode::CREATED, "{late}");
    assert_eq!(late["continued"], false, "{late}");
    assert_eq!(late["session"]["id"], first["session"]["id"]);
    studio.new_thread();
    assert_eq!(
        studio.sessions().await.len(),
        2,
        "an item has at most one live session"
    );
}
