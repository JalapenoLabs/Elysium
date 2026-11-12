// Copyright © 2026 Jalapeno Labs

//! A meeting's files and frames over the routes (#250), against Postgres, Redis, the storage
//! module's fake Bunny zone, and a staging directory of the test's own: a file attached in parts,
//! stored and read by the engine's steps, and indexed with the meeting; a frame taken from the
//! player, pinned, and carried with the excerpt over it; taking either off and undoing it; the
//! limits a meeting holds files to; and who reaches them.
//!
//! They are `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs
//! them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use diesel::{ExpressionMethods, QueryDsl};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use image::{ImageEncoder, Rgba, RgbaImage};
use secrecy::SecretString;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::database::schema::{knowledge_documents, knowledge_passages, meeting_uploads};
use crate::meeting_uploads::staging::Staging;
use crate::meetings::attachment_workflow::{
    AttachmentDeps, AttachmentFile, ReadAttachmentText, StoreAttachment,
};
use crate::models::meeting_word::tests::{built_meeting, word};
use crate::models::project::{self, NewProject, ProjectScope};
use crate::models::storage_location::{
    self, BunnyStorageRegion, NewStorageLocation, StorageProvider,
};
use crate::models::user::SYSTEM_USER_ID;
use crate::realtime::EventBus;
use crate::routes;
use crate::state::AppState;
use crate::storage::tests::{Files, PASSWORD, StoredFile, fake_storage};
use crate::test_support::{
    TEST_PERSON_ID, app_state_with_storage, migrated_database, signed_in_cookie,
};
use crate::workflows::{Done, Step, StepContext};

struct Fixture {
    state: AppState,
    connection: AsyncPgConnection,
    files: Files,
    location_id: Uuid,
    _staging: TempDir,
    cookie: String,
}

/// A signed-in person, a storage location in the fake zone chosen for attachments, and a staging
/// directory standing in for the workspace's.
async fn fixture() -> Fixture {
    let (database_url, mut connection) = migrated_database().await;
    let (storage, files) = fake_storage().await;
    let mut state = app_state_with_storage(&database_url, storage).await;
    state.events = EventBus::in_process();
    let staging = tempfile::tempdir().expect("a staging directory");
    state.upload_staging = Staging::in_directory(staging.path());
    let location = storage_location::create(
        &mut connection,
        &state.cipher,
        &NewStorageLocation {
            created_by: TEST_PERSON_ID,
            name: "Files".to_owned(),
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
    execute(
        &mut connection,
        format!(
            "UPDATE workspace_settings SET page_attachment_storage_location_id = '{}'",
            location.id
        ),
    )
    .await;
    let cookie = signed_in_cookie(&state).await;
    Fixture {
        state,
        connection,
        files,
        location_id: location.id,
        _staging: staging,
        cookie,
    }
}

struct Answer {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    bytes: Vec<u8>,
}

impl Answer {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes).unwrap_or(Value::Null)
    }
}

impl Fixture {
    async fn send(&self, method: Method, path: &str, content_type: &str, body: Vec<u8>) -> Answer {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::COOKIE, &self.cookie)
            .header(header::ORIGIN, "http://localhost:4000")
            .header(header::CONTENT_TYPE, content_type)
            .header(header::CONTENT_LENGTH, body.len())
            .body(Body::from(body))
            .expect("a request builds");
        let response = routes::router(&self.state)
            .with_state(self.state.clone())
            .oneshot(request)
            .await
            .expect("the router answers");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 24)
            .await
            .expect("the body reads")
            .to_vec();
        Answer {
            status,
            headers,
            bytes,
        }
    }

    async fn json(&self, method: Method, path: &str, body: &Value) -> Answer {
        let bytes = serde_json::to_vec(body).expect("serializes");
        self.send(method, path, "application/json", bytes).await
    }

    async fn get(&self, path: &str) -> Answer {
        self.send(Method::GET, path, "application/json", Vec::new())
            .await
    }

    /// The steps the engine runs on a finished upload of a file, as the engine builds them.
    fn steps(&self) -> (StoreAttachment, ReadAttachmentText) {
        let deps = AttachmentDeps {
            database: self.state.database.clone(),
            events: self.state.events.clone(),
            cipher: std::sync::Arc::clone(&self.state.cipher),
            storage: self.state.storage.clone(),
            workspace: unreachable_workspace(),
            staging: self.state.upload_staging.clone(),
        };
        (
            StoreAttachment::new(deps.clone()),
            ReadAttachmentText::new(deps),
        )
    }

    /// Attaches `bytes` named `name` to meeting `meeting_id` in parts, as the page does, and
    /// answers what finishing the upload answered.
    async fn attach(&self, meeting_id: Uuid, name: &str, bytes: &[u8]) -> Answer {
        let created = self
            .json(
                Method::POST,
                "/api/v1/meeting-uploads",
                &json!({
                    "meetingId": meeting_id,
                    "attachment": { "name": name, "sizeBytes": bytes.len() },
                }),
            )
            .await;
        assert_eq!(created.status, StatusCode::CREATED, "{}", created.json());
        let upload_id = created.json()["upload"]["id"]
            .as_str()
            .expect("an upload id")
            .to_owned();
        let sent = self
            .send(
                Method::PUT,
                &format!("/api/v1/meeting-uploads/{upload_id}/attachment/parts/0"),
                "application/octet-stream",
                bytes.to_vec(),
            )
            .await;
        assert_eq!(sent.status, StatusCode::OK, "{}", sent.json());
        self.json(
            Method::POST,
            &format!("/api/v1/meeting-uploads/{upload_id}/finish"),
            &json!({}),
        )
        .await
    }
}

/// A workspace whose Docker answers nothing: a plain text file is read from storage alone.
fn unreachable_workspace() -> crate::runner::workspace::WorkspaceContainer {
    let docker =
        bollard::Docker::connect_with_http("http://127.0.0.1:9", 5, bollard::API_DEFAULT_VERSION)
            .expect("a static address parses");
    crate::runner::workspace::WorkspaceContainer::new(docker, "elysium-workspace:test".to_owned())
}

async fn execute(connection: &mut AsyncPgConnection, statement: String) {
    diesel::sql_query(statement)
        .execute(connection)
        .await
        .expect("the statement runs");
}

/// What the knowledge index holds of a meeting's own document, or of a record's when `record_id` is
/// given, its passages in order; empty when it holds none.
async fn indexed(
    connection: &mut AsyncPgConnection,
    meeting_id: Option<Uuid>,
    record_id: Option<Uuid>,
) -> String {
    let query = knowledge_documents::table.select(knowledge_documents::id);
    let document = match (meeting_id, record_id) {
        (Some(meeting_id), _) => {
            query
                .filter(knowledge_documents::meeting_id.eq(meeting_id))
                .first::<Uuid>(connection)
                .await
        }
        (None, Some(record_id)) => {
            query
                .filter(knowledge_documents::timeline_record_id.eq(record_id))
                .first::<Uuid>(connection)
                .await
        }
        (None, None) => return String::new(),
    };
    let Ok(document) = document else {
        return String::new();
    };
    let passages: Vec<String> = knowledge_passages::table
        .filter(knowledge_passages::document_id.eq(document))
        .order(knowledge_passages::position.asc())
        .select(knowledge_passages::content)
        .load(connection)
        .await
        .expect("reads");
    passages.join("\n")
}

/// A PNG still, the size a browser's canvas draws a 720p video.
fn still() -> Vec<u8> {
    let picture = RgbaImage::from_pixel(1280, 720, Rgba([20, 60, 140, 255]));
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            picture.as_raw(),
            picture.width(),
            picture.height(),
            image::ExtendedColorType::Rgba8,
        )
        .expect("a PNG encodes");
    bytes
}

/// A capture whose recording is kept, so a frame may be taken of it.
async fn keep_recording(fixture: &mut Fixture, capture_id: Uuid) {
    let path = format!("meetings/recordings/{capture_id}.mp4");
    execute(
        &mut fixture.connection,
        format!(
            "UPDATE meeting_captures SET storage_location_id = '{}', recording_path = '{path}', \
             recording_content_type = 'video/mp4' WHERE id = '{capture_id}'",
            fixture.location_id
        ),
    )
    .await;
    fixture.files.lock().expect("lock").insert(
        path,
        StoredFile {
            headers: axum::http::HeaderMap::new(),
            bytes: b"a video".to_vec(),
        },
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_file_attached_in_parts_is_stored_read_and_indexed_with_its_meeting() {
    let mut fixture = fixture().await;
    let (capture, _words) = built_meeting(
        &mut fixture.connection,
        &[("Ada", "Atlas ships on Friday.")],
    )
    .await;
    let meeting_id = capture.meeting_id;
    // A meeting segmented once has its own document, which lists its files.
    execute(
        &mut fixture.connection,
        format!("UPDATE meetings SET state = 'done' WHERE id = '{meeting_id}'"),
    )
    .await;
    let text = b"Atlas launch plan\n\nShip the billing page first.";

    let finished = fixture.attach(meeting_id, "Launch plan.md", text).await;
    assert_eq!(finished.status, StatusCode::CREATED, "{}", finished.json());
    let attached = finished.json();
    let attachment_id: Uuid =
        serde_json::from_value(attached["attachment"]["id"].clone()).expect("an attachment id");
    assert_eq!(attached["attachment"]["kind"], "file");
    assert_eq!(attached["attachment"]["fileName"], "Launch plan.md");
    // Not stored yet, so not served yet: its run stores it.
    assert_eq!(attached["attachment"]["address"], Value::Null);
    assert_eq!(attached["attachment"]["textState"], "pending");
    assert_eq!(attached["entry"]["kind"], "attached");
    let upload_attachment: Option<Uuid> = meeting_uploads::table
        .filter(meeting_uploads::meeting_id.eq(meeting_id))
        .select(meeting_uploads::attachment_id)
        .first(&mut fixture.connection)
        .await
        .expect("the upload");
    assert_eq!(upload_attachment, Some(attachment_id));

    // The engine's two steps: stored where page attachments are, then read.
    let upload_id: Uuid = meeting_uploads::table
        .filter(meeting_uploads::meeting_id.eq(meeting_id))
        .select(meeting_uploads::id)
        .first(&mut fixture.connection)
        .await
        .expect("the upload");
    let (store, read) = fixture.steps();
    let file = AttachmentFile {
        attachment_id,
        upload_id: Some(upload_id),
    };
    let context = || StepContext::for_test::<StoreAttachment>(fixture.state.database.clone());
    let stored = store.run(file.clone(), context()).await.expect("stores");
    assert!(matches!(stored, Done::Succeeded { .. }), "{stored:?}");
    let kept_at = format!("meeting-attachments/{meeting_id}/{attachment_id}/Launch-plan.md");
    assert_eq!(
        fixture.files.lock().expect("lock")[&kept_at].bytes,
        text,
        "the file is kept whole, as it was sent"
    );
    // Storing again stores nothing twice.
    let again = store.run(file.clone(), context()).await.expect("stores");
    assert!(matches!(again, Done::Succeeded { .. }), "{again:?}");
    let read_text = read
        .run(
            file,
            StepContext::for_test::<ReadAttachmentText>(fixture.state.database.clone()),
        )
        .await
        .expect("reads");
    assert!(matches!(read_text, Done::Succeeded { .. }), "{read_text:?}");

    let listed = fixture
        .get(&format!("/api/v1/meetings/{meeting_id}/attachments"))
        .await;
    assert_eq!(listed.status, StatusCode::OK);
    let listed = listed.json();
    assert_eq!(listed["attachments"][0]["textState"], "read");
    let address = listed["attachments"][0]["address"]
        .as_str()
        .expect("served once stored")
        .to_owned();
    assert_eq!(
        address,
        format!("/api/v1/meetings/{meeting_id}/attachments/{attachment_id}/Launch-plan.md")
    );
    let served = fixture.get(&address).await;
    assert_eq!(served.status, StatusCode::OK);
    assert_eq!(served.bytes, text);
    assert_eq!(
        served.headers[header::CONTENT_TYPE],
        "text/markdown; charset=utf-8"
    );
    assert!(
        served.headers[header::CONTENT_DISPOSITION]
            .to_str()
            .expect("ascii")
            .starts_with("attachment;"),
        "a file that is no image downloads"
    );

    let document = indexed(&mut fixture.connection, Some(meeting_id), None).await;
    assert!(
        document.contains("Attached file: Launch plan.md"),
        "{document}"
    );
    assert!(
        document.contains("Ship the billing page first."),
        "{document}"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_frame_taken_from_the_player_travels_with_the_excerpt_it_is_pinned_in() {
    let mut fixture = fixture().await;
    let launch = project::create(
        &mut fixture.connection,
        &NewProject {
            name: "Launch".to_owned(),
            description: String::new(),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("created");
    let (capture, words) = built_meeting(
        &mut fixture.connection,
        &[
            ("Ada", "Here is the roadmap slide."),
            ("Grace", "Looks right."),
        ],
    )
    .await;
    let meeting_id = capture.meeting_id;
    keep_recording(&mut fixture, capture.id).await;
    let (roadmap, slide) = (word(&words, "roadmap"), word(&words, "slide."));

    let taken = fixture
        .send(
            Method::POST,
            &format!(
                "/api/v1/meetings/{meeting_id}/frames?captureId={}&captureMs=1500&caption=The%20Q3%20roadmap%20slide&startWordId={}&endWordId={}",
                capture.id, roadmap.id, slide.id
            ),
            "image/png",
            still(),
        )
        .await;
    assert_eq!(taken.status, StatusCode::CREATED, "{}", taken.json());
    let taken = taken.json();
    assert_eq!(taken["attachment"]["kind"], "frame");
    assert_eq!(taken["attachment"]["caption"], "The Q3 roadmap slide");
    assert_eq!(taken["attachment"]["displaysInline"], true);
    assert_eq!(taken["attachment"]["meetingMs"], 1500);
    let pins: Vec<&Value> = taken["change"]["marks"]
        .as_array()
        .expect("the marks")
        .iter()
        .filter(|mark| mark["kind"] == "pin")
        .collect();
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0]["data"]["attachmentId"], taken["attachment"]["id"]);
    assert_eq!(pins[0]["startWordId"], json!(roadmap.id));
    let pin_id = pins[0]["id"].as_str().expect("a pin").to_owned();

    // The still is kept as Elysium encoded it, and shows inline.
    let address = taken["attachment"]["address"].as_str().expect("an address");
    let served = fixture.get(address).await;
    assert_eq!(served.status, StatusCode::OK);
    assert_eq!(served.headers[header::CONTENT_TYPE], "image/webp");
    assert!(served.bytes.starts_with(b"RIFF"));
    assert!(
        served.headers[header::CONTENT_DISPOSITION]
            .to_str()
            .expect("ascii")
            .starts_with("inline;")
    );

    // An excerpt over the frame's words carries its caption to the index and its projects.
    let assigned = fixture
        .json(
            Method::POST,
            &format!("/api/v1/meetings/{meeting_id}/excerpts"),
            &json!({
                "startWordId": words[0].id,
                "endWordId": slide.id,
                "projectIds": [launch.id],
            }),
        )
        .await;
    assert_eq!(assigned.status, StatusCode::OK, "{}", assigned.json());
    let record_id: Uuid =
        serde_json::from_value(assigned.json()["recordId"].clone()).expect("a record");
    let excerpt = indexed(&mut fixture.connection, None, Some(record_id)).await;
    assert!(
        excerpt.contains("Frame shown: The Q3 roadmap slide"),
        "{excerpt}"
    );

    // Unpinned, it leaves the excerpt; its undo pins it again.
    let unpinned = fixture
        .send(
            Method::DELETE,
            &format!("/api/v1/meetings/{meeting_id}/marks/{pin_id}"),
            "application/json",
            Vec::new(),
        )
        .await;
    assert_eq!(unpinned.status, StatusCode::OK, "{}", unpinned.json());
    let excerpt = indexed(&mut fixture.connection, None, Some(record_id)).await;
    assert!(!excerpt.contains("Q3 roadmap"), "{excerpt}");
    let entry_id = unpinned.json()["entry"]["id"]
        .as_str()
        .expect("an entry")
        .to_owned();
    let undone = fixture
        .json(
            Method::POST,
            &format!("/api/v1/meetings/{meeting_id}/history/{entry_id}/undo"),
            &json!({}),
        )
        .await;
    assert_eq!(undone.status, StatusCode::OK, "{}", undone.json());
    let excerpt = indexed(&mut fixture.connection, None, Some(record_id)).await;
    assert!(
        excerpt.contains("Frame shown: The Q3 roadmap slide"),
        "{excerpt}"
    );

    // Taking the frame off takes its pin with it, and its undo puts both back.
    let attachment_id = taken["attachment"]["id"].as_str().expect("an id");
    let detached = fixture
        .send(
            Method::DELETE,
            &format!("/api/v1/meetings/{meeting_id}/attachments/{attachment_id}"),
            "application/json",
            Vec::new(),
        )
        .await;
    assert_eq!(detached.status, StatusCode::OK, "{}", detached.json());
    let detached = detached.json();
    assert_eq!(detached["entry"]["kind"], "detached");
    assert!(
        detached["change"]["marks"]
            .as_array()
            .expect("the marks")
            .iter()
            .all(|mark| mark["kind"] != "pin")
    );
    assert_eq!(fixture.get(address).await.status, StatusCode::NOT_FOUND);
    let excerpt = indexed(&mut fixture.connection, None, Some(record_id)).await;
    assert!(!excerpt.contains("Q3 roadmap"), "{excerpt}");
    let entry_id = detached["entry"]["id"].as_str().expect("an entry");
    let undone = fixture
        .json(
            Method::POST,
            &format!("/api/v1/meetings/{meeting_id}/history/{entry_id}/undo"),
            &json!({}),
        )
        .await;
    assert_eq!(undone.status, StatusCode::OK, "{}", undone.json());
    assert_eq!(fixture.get(address).await.status, StatusCode::OK);
    let excerpt = indexed(&mut fixture.connection, None, Some(record_id)).await;
    assert!(
        excerpt.contains("Frame shown: The Q3 roadmap slide"),
        "{excerpt}"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_file_is_pinned_over_an_excerpts_range_and_a_frame_with_no_range_over_its_moment() {
    let mut fixture = fixture().await;
    let (capture, words) = built_meeting(
        &mut fixture.connection,
        &[("Ada", "First the deck."), ("Grace", "Then the demo.")],
    )
    .await;
    let meeting_id = capture.meeting_id;
    keep_recording(&mut fixture, capture.id).await;
    let finished = fixture
        .attach(meeting_id, "deck.pdf", b"%PDF-1.7 a deck")
        .await;
    assert_eq!(finished.status, StatusCode::CREATED, "{}", finished.json());
    let file_id = finished.json()["attachment"]["id"].clone();

    let pinned = fixture
        .json(
            Method::POST,
            &format!("/api/v1/meetings/{meeting_id}/pins"),
            &json!({
                "attachmentId": file_id,
                "startWordId": words[0].id,
                "endWordId": word(&words, "deck.").id,
            }),
        )
        .await;
    assert_eq!(pinned.status, StatusCode::OK, "{}", pinned.json());
    assert_eq!(pinned.json()["entry"]["kind"], "marked");
    assert_eq!(pinned.json()["entry"]["data"]["kind"], "pin");

    // Grace speaks from two seconds in, so a frame at 2.5 seconds is pinned over her first word.
    let taken = fixture
        .send(
            Method::POST,
            &format!(
                "/api/v1/meetings/{meeting_id}/frames?captureId={}&captureMs=2500&caption=The%20demo",
                capture.id
            ),
            "image/png",
            still(),
        )
        .await;
    assert_eq!(taken.status, StatusCode::CREATED, "{}", taken.json());
    let pin = taken.json()["change"]["marks"]
        .as_array()
        .expect("marks")
        .iter()
        .find(|mark| mark["data"]["attachmentId"] == taken.json()["attachment"]["id"])
        .cloned()
        .expect("its pin");
    assert_eq!(pin["startWordId"], json!(word(&words, "Then").id));
    assert_eq!(pin["endWordId"], json!(word(&words, "Then").id));

    // A range given with one end alone, or a frame of another meeting's capture, is refused.
    let half = fixture
        .send(
            Method::POST,
            &format!(
                "/api/v1/meetings/{meeting_id}/frames?captureId={}&captureMs=0&caption=Half&startWordId={}",
                capture.id, words[0].id
            ),
            "image/png",
            still(),
        )
        .await;
    assert_eq!(half.status, StatusCode::BAD_REQUEST);
    let (other, _other_words) =
        built_meeting(&mut fixture.connection, &[("Ada", "Elsewhere.")]).await;
    let elsewhere = fixture
        .send(
            Method::POST,
            &format!(
                "/api/v1/meetings/{meeting_id}/frames?captureId={}&captureMs=0&caption=Elsewhere",
                other.id
            ),
            "image/png",
            still(),
        )
        .await;
    assert_eq!(elsewhere.status, StatusCode::NOT_FOUND);
    let not_an_image = fixture
        .send(
            Method::POST,
            &format!(
                "/api/v1/meetings/{meeting_id}/frames?captureId={}&captureMs=0&caption=Script",
                capture.id
            ),
            "image/png",
            b"<svg onload=alert(1)>".to_vec(),
        )
        .await;
    assert_eq!(not_an_image.status, StatusCode::BAD_REQUEST);
    assert_eq!(not_an_image.json()["code"], "meeting_frame_not_an_image");
    // Nothing a refused frame wrote stays behind with no row naming it.
    let kept: usize = fixture
        .files
        .lock()
        .expect("lock")
        .keys()
        .filter(|path| path.starts_with("meeting-attachments/"))
        .count();
    assert_eq!(
        kept, 1,
        "the demo's frame alone; the file is still in its parts"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_meeting_takes_files_up_to_the_admins_limit_and_into_a_chosen_location_alone() {
    let mut fixture = fixture().await;
    let (capture, _words) = built_meeting(&mut fixture.connection, &[("Ada", "Files.")]).await;
    let meeting_id = capture.meeting_id;

    let policy = fixture.get("/api/v1/meeting-attachments").await.json();
    assert_eq!(policy["policy"]["maxBytes"], 1_000_000_000_u64);
    assert_eq!(policy["policy"]["maxBytesCeiling"], 5_368_709_120_u64);
    assert_eq!(
        policy["policy"]["destinationId"],
        json!(fixture.location_id)
    );

    // The admin sets the largest file; past the ceiling, or none at all, is refused.
    for refused in [0_u64, 5_368_709_121] {
        let answer = fixture
            .json(
                Method::PUT,
                "/api/v1/meeting-attachments/limit",
                &json!({ "maxBytes": refused }),
            )
            .await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{refused}");
    }
    let limited = fixture
        .json(
            Method::PUT,
            "/api/v1/meeting-attachments/limit",
            &json!({ "maxBytes": 10 }),
        )
        .await;
    assert_eq!(limited.status, StatusCode::OK, "{}", limited.json());
    assert_eq!(limited.json()["policy"]["maxBytes"], 10);

    let create = |size: usize, name: &str| json!({ "meetingId": meeting_id, "attachment": { "name": name, "sizeBytes": size } });
    let too_large = fixture
        .json(
            Method::POST,
            "/api/v1/meeting-uploads",
            &create(11, "big.zip"),
        )
        .await;
    assert_eq!(too_large.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(too_large.json()["code"], "meeting_attachment_too_large");
    let fits = fixture
        .json(
            Method::POST,
            "/api/v1/meeting-uploads",
            &create(10, "LICENSE"),
        )
        .await;
    assert_eq!(fits.status, StatusCode::CREATED, "{}", fits.json());
    // An image must be the image its name says from its first part.
    let image = fixture
        .json(
            Method::POST,
            "/api/v1/meeting-uploads",
            &create(10, "fake.png"),
        )
        .await;
    let upload_id = image.json()["upload"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let part = fixture
        .send(
            Method::PUT,
            &format!("/api/v1/meeting-uploads/{upload_id}/attachment/parts/0"),
            "application/octet-stream",
            b"<html>abcd".to_vec(),
        )
        .await;
    assert_eq!(part.status, StatusCode::BAD_REQUEST);
    assert_eq!(part.json()["code"], "meeting_attachment_not_an_image");

    // With nowhere chosen to keep them, nothing is attached, neither a file nor a frame.
    execute(
        &mut fixture.connection,
        "UPDATE workspace_settings SET page_attachment_storage_location_id = NULL".to_owned(),
    )
    .await;
    let nowhere = fixture
        .json(
            Method::POST,
            "/api/v1/meeting-uploads",
            &create(5, "notes.txt"),
        )
        .await;
    assert_eq!(nowhere.status, StatusCode::CONFLICT);
    assert_eq!(nowhere.json()["code"], "meeting_attachment_no_location");
    keep_recording(&mut fixture, capture.id).await;
    let frame = fixture
        .send(
            Method::POST,
            &format!(
                "/api/v1/meetings/{meeting_id}/frames?captureId={}&captureMs=0&caption=Nowhere",
                capture.id
            ),
            "image/png",
            still(),
        )
        .await;
    assert_eq!(frame.status, StatusCode::CONFLICT);
    assert_eq!(frame.json()["code"], "meeting_attachment_no_location");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn only_whoever_reads_a_meeting_reaches_its_files_and_frames() {
    let mut fixture = fixture().await;
    let (capture, words) = built_meeting(&mut fixture.connection, &[("Ada", "Private.")]).await;
    let meeting_id = capture.meeting_id;
    keep_recording(&mut fixture, capture.id).await;
    let taken = fixture
        .send(
            Method::POST,
            &format!(
                "/api/v1/meetings/{meeting_id}/frames?captureId={}&captureMs=0&caption=Ours",
                capture.id
            ),
            "image/png",
            still(),
        )
        .await;
    assert_eq!(taken.status, StatusCode::CREATED, "{}", taken.json());
    let taken = taken.json();
    let attachment_id = taken["attachment"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let address = taken["attachment"]["address"]
        .as_str()
        .expect("an address")
        .to_owned();

    // Someone else's meeting, as far as the test person is concerned.
    execute(
        &mut fixture.connection,
        format!("UPDATE meetings SET created_by = '{SYSTEM_USER_ID}' WHERE id = '{meeting_id}'"),
    )
    .await;
    let attachments = format!("/api/v1/meetings/{meeting_id}/attachments");
    assert_eq!(
        fixture.get(&attachments).await.status,
        StatusCode::NOT_FOUND
    );
    let served = fixture.get(&address).await;
    assert_eq!(served.status, StatusCode::NOT_FOUND);
    assert!(!served.bytes.starts_with(b"RIFF"));
    let removed = fixture
        .send(
            Method::DELETE,
            &format!("{attachments}/{attachment_id}"),
            "application/json",
            Vec::new(),
        )
        .await;
    assert_eq!(removed.status, StatusCode::NOT_FOUND);
    let pinned = fixture
        .json(
            Method::POST,
            &format!("/api/v1/meetings/{meeting_id}/pins"),
            &json!({
                "attachmentId": attachment_id,
                "startWordId": words[0].id,
                "endWordId": words[0].id,
            }),
        )
        .await;
    assert_eq!(pinned.status, StatusCode::NOT_FOUND);
    let frame = fixture
        .send(
            Method::POST,
            &format!(
                "/api/v1/meetings/{meeting_id}/frames?captureId={}&captureMs=0&caption=Theirs",
                capture.id
            ),
            "image/png",
            still(),
        )
        .await;
    assert_eq!(frame.status, StatusCode::NOT_FOUND);
    let upload = fixture
        .json(
            Method::POST,
            "/api/v1/meeting-uploads",
            &json!({ "meetingId": meeting_id, "attachment": { "name": "a.txt", "sizeBytes": 3 } }),
        )
        .await;
    assert_eq!(upload.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_meeting_merged_away_takes_no_file_frame_or_pin() {
    let mut fixture = fixture().await;
    let (capture, words) = built_meeting(&mut fixture.connection, &[("Ada", "Merged.")]).await;
    let (kept, _kept_words) = built_meeting(&mut fixture.connection, &[("Ada", "Kept.")]).await;
    let meeting_id = capture.meeting_id;
    keep_recording(&mut fixture, capture.id).await;
    let taken = fixture
        .send(
            Method::POST,
            &format!(
                "/api/v1/meetings/{meeting_id}/frames?captureId={}&captureMs=0&caption=Before",
                capture.id
            ),
            "image/png",
            still(),
        )
        .await;
    assert_eq!(taken.status, StatusCode::CREATED, "{}", taken.json());
    let attachment_id = taken.json()["attachment"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    execute(
        &mut fixture.connection,
        format!(
            "UPDATE meetings SET merged_into = '{}' WHERE id = '{meeting_id}'",
            kept.meeting_id
        ),
    )
    .await;

    let frame = fixture
        .send(
            Method::POST,
            &format!(
                "/api/v1/meetings/{meeting_id}/frames?captureId={}&captureMs=0&caption=After",
                capture.id
            ),
            "image/png",
            still(),
        )
        .await;
    let pinned = fixture
        .json(
            Method::POST,
            &format!("/api/v1/meetings/{meeting_id}/pins"),
            &json!({
                "attachmentId": attachment_id,
                "startWordId": words[0].id,
                "endWordId": words[0].id,
            }),
        )
        .await;
    let detached = fixture
        .send(
            Method::DELETE,
            &format!("/api/v1/meetings/{meeting_id}/attachments/{attachment_id}"),
            "application/json",
            Vec::new(),
        )
        .await;
    let upload = fixture
        .json(
            Method::POST,
            "/api/v1/meeting-uploads",
            &json!({ "meetingId": meeting_id, "attachment": { "name": "a.txt", "sizeBytes": 3 } }),
        )
        .await;
    for answer in [frame, pinned, detached, upload] {
        assert_eq!(answer.status, StatusCode::CONFLICT, "{}", answer.json());
        assert_eq!(answer.json()["code"], "meeting_merged");
    }
    // What it held is still read where its page shows it.
    assert_eq!(
        fixture
            .get(&format!("/api/v1/meetings/{meeting_id}/attachments"))
            .await
            .json()["attachments"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}
