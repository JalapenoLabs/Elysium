// Copyright © 2026 Jalapeno Labs

//! Uploading meetings over the routes (#233), against Postgres, Redis, the storage module's fake
//! Bunny zone, and a staging directory of the test's own: a recording of several parts streamed
//! through to storage by the meeting's fetch, a transcript alone made a meeting in words alone,
//! what is refused and when, a capture added to a meeting, and where recordings go by default.
//!
//! Those are `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs
//! them.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE, COOKIE, ORIGIN};
use axum::http::{Method, Request, StatusCode};
use bytes::Bytes;
use diesel::{ExpressionMethods, QueryDsl};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use futures_util::stream;
use secrecy::SecretString;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::database::schema::meeting_uploads;
use crate::meeting_uploads::PART_BYTES;
use crate::meeting_uploads::staging::Staging;
use crate::meetings::fetch::{FetchMeeting, MeetingToFetch, RecordedMeeting};
use crate::meetings::sources::Sources;
use crate::meetings::workflow;
use crate::models::meeting::{self, MeetingState};
use crate::models::meeting_capture;
use crate::models::meeting_upload;
use crate::models::project::ProjectScope;
use crate::models::storage_location::{
    self, BunnyStorageRegion, NewStorageLocation, StorageLocation, StorageProvider,
};
use crate::models::workflow_run::{self, WorkflowRunState};
use crate::routes;
use crate::state::AppState;
use crate::storage::tests::{Files, PASSWORD, fake_storage};
use crate::test_support::{
    TEST_PERSON_ID, app_state_with_storage, migrated_database, signed_in_cookie,
};
use crate::workflows::{Done, Step, StepContext};

struct Fixture {
    state: AppState,
    connection: AsyncPgConnection,
    database_url: SecretString,
    files: Files,
    location: StorageLocation,
    staging: TempDir,
    cookie: String,
}

/// A signed-in person, a storage location in the fake zone, and a staging directory standing in
/// for the workspace's.
async fn fixture() -> Fixture {
    let (database_url, mut connection) = migrated_database().await;
    let (storage, files) = fake_storage().await;
    let mut state = app_state_with_storage(&database_url, storage).await;
    let staging = tempfile::tempdir().expect("a staging directory");
    state.upload_staging = Staging::in_directory(staging.path());
    let location = storage_location::create(
        &mut connection,
        &state.cipher,
        &NewStorageLocation {
            created_by: TEST_PERSON_ID,
            name: "Recordings".to_owned(),
            provider: StorageProvider::Bunny {
                zone: "files".to_owned(),
                region: BunnyStorageRegion::Frankfurt,
            },
            path_prefix: String::new(),
            storage_limit_bytes: None,
            access_key: SecretString::from(PASSWORD),
            projects: ProjectScope::All,
        },
    )
    .await
    .expect("created");
    let cookie = signed_in_cookie(&state).await;
    Fixture {
        state,
        connection,
        database_url,
        files,
        location,
        staging,
        cookie,
    }
}

struct Answer {
    status: StatusCode,
    body: Value,
}

impl Fixture {
    fn router(&self) -> Router {
        routes::router(&self.state).with_state(self.state.clone())
    }

    async fn send(&self, method: Method, path: &str, body: Body, length: Option<usize>) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(COOKIE, &self.cookie)
            .header(ORIGIN, "http://localhost:4000");
        if let Some(length) = length {
            request = request.header(CONTENT_LENGTH, length);
        }
        let response = self
            .router()
            .oneshot(request.body(body).expect("a request builds"))
            .await
            .expect("the router answers");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("the body reads");
        Answer {
            status,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    async fn json(&self, method: Method, path: &str, body: &Value) -> Answer {
        let bytes = serde_json::to_vec(body).expect("serializes");
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header(COOKIE, &self.cookie)
            .header(ORIGIN, "http://localhost:4000")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(bytes))
            .expect("a request builds");
        let response = self
            .router()
            .oneshot(request)
            .await
            .expect("the router answers");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("the body reads");
        Answer {
            status,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    async fn get(&self, path: &str) -> Answer {
        self.send(Method::GET, path, Body::empty(), None).await
    }

    /// Sends `bytes` as part `index` of upload `id`'s file of `kind`, streamed in small chunks as
    /// a browser's body arrives, never as one buffer.
    async fn put_part(&self, id: &str, kind: &str, index: u64, bytes: Bytes) -> Answer {
        let length = bytes.len();
        let chunks: Vec<Result<Bytes, std::io::Error>> = (0..length)
            .step_by(64 * 1024)
            .map(|start| Ok(bytes.slice(start..length.min(start + 64 * 1024))))
            .collect();
        self.send(
            Method::PUT,
            &format!("/api/v1/meeting-uploads/{id}/{kind}/parts/{index}"),
            Body::from_stream(stream::iter(chunks)),
            Some(length),
        )
        .await
    }

    /// The meeting workflow's fetch, storing through the fake zone and reading this fixture's
    /// staging.
    async fn fetch_step(&self) -> FetchMeeting {
        let database = crate::connections::connect_postgres(&self.database_url, 4)
            .await
            .expect("a pool");
        FetchMeeting::for_uploads(
            database,
            Arc::clone(&self.state.cipher),
            Sources::new(reqwest::Client::new()),
            self.state.storage.clone(),
            Staging::in_directory(self.staging.path()),
        )
    }
}

/// A recording's bytes: an MP4's opening box, then a pattern that changes every byte, so a part
/// joined out of place or twice would change the hash.
fn recording_bytes(length: usize) -> Bytes {
    let mut bytes = b"\0\0\0\x20ftypisom\0\0\x02\0isomiso2".to_vec();
    bytes.extend((bytes.len()..length).map(|index| u8::try_from(index % 251).expect("under 251")));
    Bytes::from(bytes)
}

fn id_of(answer: &Answer, field: &str) -> String {
    answer.body[field]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("an id in {:?}", answer.body))
        .to_owned()
}

fn received(answer: &Answer, kind: &str) -> u64 {
    answer.body["upload"]["files"]
        .as_array()
        .expect("files")
        .iter()
        .find(|file| file["kind"] == kind)
        .and_then(|file| file["receivedBytes"].as_u64())
        .expect("the file's received bytes")
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one upload from its first part to its recording in storage, read in order"
)]
async fn a_large_recording_streams_in_parts_through_to_storage() {
    let mut fixture = fixture().await;
    let part = usize::try_from(PART_BYTES).expect("fits");
    // Two whole parts and a short last one: more than any one request may carry.
    let recording = recording_bytes(2 * part + 1_000);

    let created = fixture
        .json(
            Method::POST,
            "/api/v1/meeting-uploads",
            &json!({
                "title": "Board call",
                "startedAt": "2026-11-09T15:00:00Z",
                "storageLocationId": fixture.location.id,
                "recording": {
                    "name": "C:\\Recordings\\Board call.MP4",
                    "sizeBytes": recording.len(),
                    "modifiedAt": "2026-11-09T16:00:00Z",
                },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let upload = id_of(&created, "upload");
    assert_eq!(created.body["upload"]["partBytes"], PART_BYTES);
    assert_eq!(
        created.body["upload"]["files"][0]["fileName"],
        "Board call.mp4"
    );

    // A part past the next one is refused, and says nothing arrived.
    let ahead = fixture
        .put_part(&upload, "recording", 1, recording.slice(part..2 * part))
        .await;
    assert_eq!(ahead.status, StatusCode::CONFLICT);
    assert_eq!(ahead.body["code"], "meeting_upload_part_ahead");

    // A part is exactly its share of the file: more is refused before any of it is read.
    let whole = fixture
        .send(
            Method::PUT,
            &format!("/api/v1/meeting-uploads/{upload}/recording/parts/0"),
            Body::from(recording.clone()),
            Some(recording.len()),
        )
        .await;
    assert_eq!(whole.status, StatusCode::BAD_REQUEST);
    assert_eq!(whole.body["code"], "meeting_upload_part_length");

    let first = fixture
        .put_part(&upload, "recording", 0, recording.slice(..part))
        .await;
    assert_eq!(first.status, StatusCode::OK, "{:?}", first.body);
    assert_eq!(received(&first, "recording"), u64::from(PART_BYTES));

    // The same part again, as after a dropped answer: acknowledged, and counted once.
    let again = fixture
        .put_part(&upload, "recording", 0, recording.slice(..part))
        .await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(received(&again, "recording"), u64::from(PART_BYTES));

    // Finishing before the rest arrived is refused, and the page resumes from what it reads.
    let early = fixture
        .send(
            Method::POST,
            &format!("/api/v1/meeting-uploads/{upload}/finish"),
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(early.body["code"], "meeting_upload_incomplete");
    let resumed = fixture
        .get(&format!("/api/v1/meeting-uploads/{upload}"))
        .await;
    let next = received(&resumed, "recording") / u64::from(PART_BYTES);
    assert_eq!(next, 1);
    let listed = fixture.get("/api/v1/meeting-uploads").await;
    assert_eq!(listed.body["uploads"][0]["id"], upload.as_str());

    for index in next..3 {
        let start = usize::try_from(index).expect("fits") * part;
        let end = recording.len().min(start + part);
        let sent = fixture
            .put_part(&upload, "recording", index, recording.slice(start..end))
            .await;
        assert_eq!(sent.status, StatusCode::OK, "{:?}", sent.body);
    }

    let finished = fixture
        .send(
            Method::POST,
            &format!("/api/v1/meeting-uploads/{upload}/finish"),
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(finished.status, StatusCode::CREATED, "{:?}", finished.body);
    let meeting_id: Uuid = id_of(&finished, "meeting").parse().expect("an id");
    assert_eq!(finished.body["meeting"]["title"], "Board call");
    assert_eq!(finished.body["meeting"]["sourceKind"], "upload");
    assert_eq!(finished.body["meeting"]["state"], "pending");
    assert_eq!(finished.body["meeting"]["durationMs"], Value::Null);

    // Its run is started by the person, from the fetch of the capture the upload became.
    let run = workflow_run::newest(
        &mut fixture.connection,
        workflow::workflow().name(),
        workflow::SOURCE_KIND,
        &meeting_id.to_string(),
    )
    .await
    .expect("reads")
    .expect("a run");
    assert_eq!(run.created_by, TEST_PERSON_ID);
    assert_eq!(run.state, WorkflowRunState::Queued);
    let staged = meeting_upload::find_owned(
        &mut fixture.connection,
        upload.parse().expect("an id"),
        TEST_PERSON_ID,
    )
    .await
    .expect("found");
    let capture_id = staged.upload.capture_id.expect("finished as a capture");
    assert_eq!(
        run.input,
        json!({ "meetingId": meeting_id, "captureId": capture_id })
    );
    // Finishing again answers the same meeting.
    let twice = fixture
        .send(
            Method::POST,
            &format!("/api/v1/meeting-uploads/{upload}/finish"),
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(twice.status, StatusCode::OK);
    assert_eq!(twice.body["meeting"]["id"], meeting_id.to_string());

    // The fetch joins the parts as they stream into storage, named by the hash of their bytes.
    let step = fixture.fetch_step().await;
    let context = StepContext::for_test::<FetchMeeting>(step_database(&fixture).await);
    let log = context.log.clone();
    let done = step
        .run(
            MeetingToFetch {
                meeting_id,
                capture_id: Some(capture_id),
            },
            context,
        )
        .await
        .expect("the fetch succeeds");
    let Done::Succeeded {
        output: RecordedMeeting { recording, .. },
        summary,
    } = done
    else {
        panic!("an upload's fetch succeeds");
    };
    let hash = hex::encode(Sha256::digest(recording_bytes(2 * part + 1_000)));
    let path = format!("meetings/{meeting_id}/captures/{capture_id}/{hash}.mp4");
    let kept = recording.expect("the recording is kept");
    assert_eq!(kept.path, path);
    assert_eq!(kept.content_type.as_deref(), Some("video/mp4"));
    assert_eq!(
        summary.as_deref(),
        Some("Stored 0 transcript lines and the recording from the upload")
    );
    let stored_bytes = fixture
        .files
        .lock()
        .expect("the fake's files")
        .get(&path)
        .map(|file| file.bytes.clone())
        .expect("the recording reached storage");
    assert_eq!(stored_bytes.len(), 2 * part + 1_000);
    assert_eq!(hex::encode(Sha256::digest(&stored_bytes)), hash);
    let capture = meeting_capture::find(&mut fixture.connection, capture_id)
        .await
        .expect("found");
    assert_eq!(capture.recording_hash.as_deref(), Some(hash.as_str()));
    assert_eq!(capture.storage_location_id, Some(fixture.location.id));
    let recorded = meeting::find(&mut fixture.connection, meeting_id)
        .await
        .expect("found");
    assert_eq!(recorded.state(), MeetingState::Recorded);
    // What was staged is gone, and the run's log names no file or title.
    assert!(!fixture.staging.path().join(&upload).exists());
    let log = log.unstored();
    assert!(!log.contains("Board call"), "{log}");
}

/// A pool for a step's context, on the fixture's database.
async fn step_database(fixture: &Fixture) -> crate::database::Pool {
    crate::connections::connect_postgres(&fixture.database_url, 4)
        .await
        .expect("a pool")
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_transcript_alone_with_no_times_is_a_meeting_in_words_alone() {
    let mut fixture = fixture().await;
    let transcript = Bytes::from_static(
        b"# Weekly sync\n\n**Ada:** Launch is Friday.\n**Grace:** Agreed, and the deck is ready.\n",
    );

    let created = fixture
        .json(
            Method::POST,
            "/api/v1/meeting-uploads",
            &json!({
                "title": "Weekly sync",
                "startedAt": "2026-11-09T15:00:00+01:00",
                // A transcript alone keeps nothing in storage, whatever is named.
                "storageLocationId": fixture.location.id,
                "transcript": { "name": "sync notes.md", "sizeBytes": transcript.len() },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    assert_eq!(created.body["upload"]["storageLocationId"], Value::Null);
    let upload = id_of(&created, "upload");
    let sent = fixture
        .put_part(&upload, "transcript", 0, transcript.clone())
        .await;
    assert_eq!(sent.status, StatusCode::OK, "{:?}", sent.body);

    let finished = fixture
        .send(
            Method::POST,
            &format!("/api/v1/meeting-uploads/{upload}/finish"),
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(finished.status, StatusCode::CREATED, "{:?}", finished.body);
    let meeting_id: Uuid = id_of(&finished, "meeting").parse().expect("an id");
    assert_eq!(finished.body["meeting"]["durationMs"], Value::Null);
    assert_eq!(
        finished.body["meeting"]["participants"],
        json!(["Ada", "Grace"])
    );
    assert_eq!(finished.body["meeting"]["hasRecording"], false);
    assert_eq!(
        finished.body["meeting"]["startedAt"], "2026-11-09T14:00:00Z",
        "stored as UTC"
    );

    let step = fixture.fetch_step().await;
    let context = StepContext::for_test::<FetchMeeting>(step_database(&fixture).await);
    let done = step
        .run(
            MeetingToFetch {
                meeting_id,
                capture_id: None,
            },
            context,
        )
        .await
        .expect("the fetch succeeds");
    let Done::Succeeded { output, .. } = done else {
        panic!("an upload's fetch succeeds");
    };
    assert_eq!(output.recording, None);
    let capture = meeting_capture::first_of(&mut fixture.connection, meeting_id)
        .await
        .expect("found");
    assert_eq!(
        capture.transcript,
        json!([
            { "startMs": 0, "endMs": 0, "speaker": "Ada", "text": "Launch is Friday." },
            { "startMs": 0, "endMs": 0, "speaker": "Grace", "text": "Agreed, and the deck is ready." },
        ])
    );
    assert_eq!(capture.transcript_hash, output.transcript_hash);
    assert!(fixture.files.lock().expect("the fake's files").is_empty());
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "every refusal an upload meets, from its start to its finish, read as a table"
)]
async fn what_a_meeting_does_not_take_is_refused_before_it_is_kept() {
    let fixture = fixture().await;
    let refusal = async |fixture: &Fixture, body: Value| {
        let answer = fixture
            .json(Method::POST, "/api/v1/meeting-uploads", &body)
            .await;
        (answer.status, answer.body["code"].clone())
    };
    let new_meeting = |file: Value| {
        json!({
            "title": "Call",
            "startedAt": "2026-11-09T15:00:00Z",
            "storageLocationId": fixture.location.id,
            "recording": file,
        })
    };

    for (body, status, code) in [
        (
            json!({ "title": "Call", "startedAt": "2026-11-09T15:00:00Z" }),
            StatusCode::BAD_REQUEST,
            "meeting_upload_nothing",
        ),
        (
            new_meeting(json!({ "name": "call.webm", "sizeBytes": 10 })),
            StatusCode::BAD_REQUEST,
            "meeting_upload_type",
        ),
        (
            new_meeting(json!({ "name": "call.mp4", "sizeBytes": 0 })),
            StatusCode::BAD_REQUEST,
            "meeting_upload_empty",
        ),
        (
            new_meeting(json!({ "name": "call.mp4", "sizeBytes": 6_000_000_000_u64 })),
            StatusCode::PAYLOAD_TOO_LARGE,
            "meeting_upload_too_large",
        ),
        (
            json!({
                "title": "Call",
                "startedAt": "2026-11-09T15:00:00Z",
                "recording": { "name": "call.mp4", "sizeBytes": 10 },
            }),
            StatusCode::BAD_REQUEST,
            "meeting_upload_location",
        ),
        (
            json!({ "title": " ", "startedAt": "2026-11-09T15:00:00Z", "transcript": { "name": "a.vtt", "sizeBytes": 3 } }),
            StatusCode::BAD_REQUEST,
            "meeting_upload_title",
        ),
    ] {
        assert_eq!(
            refusal(&fixture, body.clone()).await,
            (status, json!(code)),
            "{body}"
        );
    }
    // A time with no offset is refused, as every request's is.
    let offsetless = refusal(
        &fixture,
        json!({ "title": "Call", "startedAt": "2026-11-09T15:00:00", "transcript": { "name": "a.vtt", "sizeBytes": 3 } }),
    )
    .await;
    assert_eq!(offsetless.0, StatusCode::BAD_REQUEST);

    // A recording whose first bytes are not its format keeps nothing.
    let created = fixture
        .json(
            Method::POST,
            "/api/v1/meeting-uploads",
            &new_meeting(json!({ "name": "call.mp4", "sizeBytes": 20 })),
        )
        .await;
    let upload = id_of(&created, "upload");
    let not_media = fixture
        .put_part(
            &upload,
            "recording",
            0,
            Bytes::from_static(b"<html><script>x()<"),
        )
        .await;
    assert_eq!(not_media.body["code"], "meeting_upload_part_length");
    let not_media = fixture
        .put_part(
            &upload,
            "recording",
            0,
            Bytes::from_static(b"<html><script>x()</s"),
        )
        .await;
    assert_eq!(not_media.status, StatusCode::BAD_REQUEST);
    assert_eq!(not_media.body["code"], "meeting_upload_not_media");
    let unchanged = fixture
        .get(&format!("/api/v1/meeting-uploads/{upload}"))
        .await;
    assert_eq!(received(&unchanged, "recording"), 0);

    // An upload nobody made is not there.
    assert_eq!(
        fixture
            .get(&format!("/api/v1/meeting-uploads/{}", Uuid::now_v7()))
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // A transcript that is not text is refused as its last part arrives, before anything sent
    // after it, and the upload stays, to cancel.
    let created = fixture
        .json(
            Method::POST,
            "/api/v1/meeting-uploads",
            &json!({
                "title": "Call",
                "startedAt": "2026-11-09T15:00:00Z",
                "transcript": { "name": "call.vtt", "sizeBytes": 8 },
            }),
        )
        .await;
    let upload = id_of(&created, "upload");
    let not_text = fixture
        .put_part(
            &upload,
            "transcript",
            0,
            Bytes::from_static(b"\0\0\0\x20ftyp"),
        )
        .await;
    assert_eq!(not_text.status, StatusCode::BAD_REQUEST);
    assert_eq!(not_text.body["code"], "meeting_upload_not_text");
    let uncounted = fixture
        .get(&format!("/api/v1/meeting-uploads/{upload}"))
        .await;
    assert_eq!(received(&uncounted, "transcript"), 0);
    let finished = fixture
        .send(
            Method::POST,
            &format!("/api/v1/meeting-uploads/{upload}/finish"),
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(finished.body["code"], "meeting_upload_incomplete");
    assert!(fixture.staging.path().join(&upload).exists());
    let cancelled = fixture
        .send(
            Method::DELETE,
            &format!("/api/v1/meeting-uploads/{upload}"),
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(cancelled.status, StatusCode::NO_CONTENT);
    assert!(!fixture.staging.path().join(&upload).exists());
    assert_eq!(
        fixture
            .get(&format!("/api/v1/meeting-uploads/{upload}"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_capture_joins_a_meeting_once_its_run_is_done() {
    let mut fixture = fixture().await;
    let (meeting, _first) = crate::models::meeting::tests::stored(&mut fixture.connection).await;
    let started = crate::workflows::start(
        &mut fixture.connection,
        &fixture.state.events,
        workflow::run_for(meeting.id),
    )
    .await
    .expect("its run starts");
    let transcript =
        Bytes::from_static(b"WEBVTT\n\n00:00:01.000 --> 00:00:03.000\n<v Ada>Hello</v>\n");
    let capture_upload = json!({
        "meetingId": meeting.id,
        "transcript": { "name": "teams.vtt", "sizeBytes": transcript.len() },
    });

    let busy = fixture
        .json(Method::POST, "/api/v1/meeting-uploads", &capture_upload)
        .await;
    assert_eq!(busy.status, StatusCode::CONFLICT);
    assert_eq!(busy.body["code"], "meeting_processing");

    // A run that failed waits for its person, and says so.
    let set_run = async |connection: &mut AsyncPgConnection, state: WorkflowRunState| {
        diesel::update(crate::database::schema::workflow_runs::table.find(started.settled.run.id))
            .set((
                crate::database::schema::workflow_runs::state.eq(state),
                crate::database::schema::workflow_runs::finished_at.eq(Some(chrono::Utc::now())),
                crate::database::schema::workflow_runs::error
                    .eq((state == WorkflowRunState::Failed).then(|| "The fetch failed".to_owned())),
            ))
            .execute(connection)
            .await
            .expect("moves the run");
    };
    set_run(&mut fixture.connection, WorkflowRunState::Failed).await;
    let failed = fixture
        .json(Method::POST, "/api/v1/meeting-uploads", &capture_upload)
        .await;
    assert_eq!(failed.status, StatusCode::CONFLICT);
    assert_eq!(failed.body["code"], "meeting_run_failed");

    // Its run done, the meeting takes the capture.
    set_run(&mut fixture.connection, WorkflowRunState::Succeeded).await;
    let created = fixture
        .json(Method::POST, "/api/v1/meeting-uploads", &capture_upload)
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let upload = id_of(&created, "upload");
    fixture.put_part(&upload, "transcript", 0, transcript).await;
    let finished = fixture
        .send(
            Method::POST,
            &format!("/api/v1/meeting-uploads/{upload}/finish"),
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(finished.status, StatusCode::CREATED, "{:?}", finished.body);
    assert_eq!(finished.body["meeting"]["id"], meeting.id.to_string());
    let staged = meeting_upload::find_owned(
        &mut fixture.connection,
        upload.parse().expect("an id"),
        TEST_PERSON_ID,
    )
    .await
    .expect("found");
    let capture = meeting_capture::find(
        &mut fixture.connection,
        staged.upload.capture_id.expect("a capture"),
    )
    .await
    .expect("found");
    assert_eq!(capture.meeting_id, meeting.id);
    assert_eq!(capture.source_kind, "upload");
    assert_eq!(
        staged.upload.transcript,
        Some(json!([{ "startMs": 1000, "endMs": 3000, "speaker": "Ada", "text": "Hello" }]))
    );

    // Someone else's meeting takes nothing from this person.
    let other = json!({
        "meetingId": Uuid::now_v7(),
        "transcript": { "name": "teams.vtt", "sizeBytes": 10 },
    });
    let missing = fixture
        .json(Method::POST, "/api/v1/meeting-uploads", &other)
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn the_policy_says_what_a_meeting_takes_and_where_recordings_go() {
    let fixture = fixture().await;

    let policy = fixture.get("/api/v1/meeting-uploads/policy").await;

    assert_eq!(policy.status, StatusCode::OK);
    assert_eq!(policy.body["policy"]["partBytes"], PART_BYTES);
    assert_eq!(
        policy.body["policy"]["recording"]["extensions"],
        json!(["mp3", "wav", "mp4", "mov"])
    );
    assert_eq!(
        policy.body["policy"]["transcript"]["maxBytes"],
        crate::meeting_uploads::MAX_TRANSCRIPT_BYTES
    );
    // No meeting source keeps recordings yet, so the person chooses.
    assert_eq!(
        policy.body["policy"]["defaultStorageLocationId"],
        Value::Null
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn an_upload_left_for_a_week_is_swept_and_a_finished_one_stays() {
    let (_url, mut connection) = migrated_database().await;
    let new_upload = |title: &str| meeting_upload::NewMeetingUpload {
        id: Uuid::now_v7(),
        meeting_id: None,
        title: Some(title.to_owned()),
        started_at: Some(chrono::Utc::now()),
        storage_location_id: None,
        part_bytes: 8,
        created_by: TEST_PERSON_ID,
    };
    let transcript = || {
        vec![meeting_upload::NewUploadFile {
            kind: crate::meeting_uploads::FileKind::Transcript,
            file_name: "a.txt".to_owned(),
            size_bytes: 3,
            modified_at: None,
        }]
    };
    let stale = meeting_upload::create(&mut connection, new_upload("Stale"), transcript())
        .await
        .expect("created");
    let fresh = meeting_upload::create(&mut connection, new_upload("Fresh"), transcript())
        .await
        .expect("created");
    diesel::update(meeting_uploads::table.find(stale.upload.id))
        .set(meeting_uploads::updated_at.eq(chrono::Utc::now()
            - crate::meeting_uploads::STALE_AFTER
            - chrono::TimeDelta::hours(1)))
        .execute(&mut connection)
        .await
        .expect("ages");

    // A part arriving moves an upload, so one still arriving is never stale.
    let counted = meeting_upload::record_part(
        &mut connection,
        (
            fresh.upload.id,
            crate::meeting_uploads::FileKind::Transcript,
        ),
        0,
        3,
    )
    .await
    .expect("records");
    assert!(counted);
    let recounted = meeting_upload::record_part(
        &mut connection,
        (
            fresh.upload.id,
            crate::meeting_uploads::FileKind::Transcript,
        ),
        0,
        3,
    )
    .await
    .expect("records");
    assert!(!recounted, "the same part counts once");

    let swept = meeting_upload::delete_stale(
        &mut connection,
        chrono::Utc::now() - crate::meeting_uploads::STALE_AFTER,
    )
    .await
    .expect("sweeps");

    assert_eq!(swept, vec![stale.upload.id]);
    meeting_upload::find_owned(&mut connection, fresh.upload.id, TEST_PERSON_ID)
        .await
        .expect("an upload still arriving stays");
}
