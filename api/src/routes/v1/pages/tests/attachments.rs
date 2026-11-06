// Copyright © 2026 Jalapeno Labs

//! Page attachments over the routes (#202), against Postgres, Redis, and the storage module's fake
//! Bunny zone: what a page takes, what its address answers and to whom, and what happens to a file
//! as its page changes.
//!
//! Those are `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs
//! them.

use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_SECURITY_POLICY, CONTENT_TYPE,
    COOKIE, ORIGIN, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::middleware::{from_fn, from_fn_with_state};
use bytes::Bytes;
use diesel::QueryableByName;
use diesel::sql_types::Jsonb;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use futures_util::stream;
use secrecy::SecretString;
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use super::{ELYSIA_USER_ID, author, call, create, kinds, set_role};
use crate::crypto::Cipher;
use crate::middleware::{request_timeout, security_headers};
use crate::models::agent::{Agent, Author};
use crate::models::company_doc::Section;
use crate::models::learning::{self, LearningContent};
use crate::models::page::{self, LEARNINGS_PAGE_ID, PageError};
use crate::models::page_attachment::{self, MAX_ATTACHMENTS_PER_PAGE, StoredFile};
use crate::models::project::ProjectScope;
use crate::models::storage_location::{
    self, BunnyStorageRegion, NewStorageLocation, StorageLocation, StorageProvider,
};
use crate::models::user::{self, WorkspaceSettingsChanges};
use crate::page_attachments::MAX_ATTACHMENT_BYTES;
use crate::routes;
use crate::routes::v1::pages::upload_attachment::{self, Upload};
use crate::state::AppState;
use crate::storage::tests::{Files, PASSWORD, fake_storage};
use crate::test_support::{
    TEST_PERSON_ID, app_state_with_storage, migrated_database, signed_in_cookie,
};

/// A complete PNG's first bytes, which is all the signature check reads.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR and the rest of an image";

struct Fixture {
    state: AppState,
    connection: AsyncPgConnection,
    files: Files,
    location: StorageLocation,
    page: Uuid,
}

async fn location_in_the_fake_zone(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    name: &str,
) -> StorageLocation {
    storage_location::create(
        connection,
        cipher,
        &NewStorageLocation {
            created_by: TEST_PERSON_ID,
            name: name.to_owned(),
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
    .expect("created")
}

async fn choose(connection: &mut AsyncPgConnection, location: Option<Uuid>) {
    user::update_workspace_settings(
        connection,
        WorkspaceSettingsChanges {
            page_attachment_storage_location_id: Some(location),
            ..WorkspaceSettingsChanges::default()
        },
        TEST_PERSON_ID,
    )
    .await
    .expect("chosen");
}

/// A page, and a location chosen for attachments in the fake zone.
async fn fixture() -> Fixture {
    let (url, mut connection) = migrated_database().await;
    let (storage, files) = fake_storage().await;
    let state = app_state_with_storage(&url, storage).await;
    let location = location_in_the_fake_zone(&mut connection, &state.cipher, "Files").await;
    choose(&mut connection, Some(location.id)).await;
    let page = create(&mut connection, None, "Deploying").await;
    Fixture {
        state,
        connection,
        files,
        location,
        page,
    }
}

/// The API's router, with the layers around it that `build_router` puts around every route and
/// these tests are about.
fn router(state: &AppState) -> Router {
    routes::router(state)
        .with_state(state.clone())
        .layer(from_fn(security_headers::apply))
}

struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    bytes: Bytes,
}

impl Answer {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes).unwrap_or(Value::Null)
    }

    fn header(&self, name: axum::http::HeaderName) -> &str {
        self.headers
            .get(name)
            .map_or("", |value| value.to_str().expect("a plain header"))
    }
}

async fn answer(app: Router, request: Request<Body>) -> Answer {
    let response = app.oneshot(request).await.expect("the router answers");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("the body reads");
    Answer {
        status,
        headers,
        bytes,
    }
}

fn upload_request(cookie: &str, page: Uuid, name: &str, bytes: &[u8]) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(format!("/api/v1/pages/{page}/attachments?name={name}"))
        .header(COOKIE, cookie)
        .header(ORIGIN, "http://localhost:4000")
        // What the browser claims is believed nowhere: this says HTML for every file sent here.
        .header(CONTENT_TYPE, "text/html")
        .header(CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes.to_vec()))
        .expect("a request builds")
}

async fn upload(state: &AppState, page: Uuid, name: &str, bytes: &[u8]) -> Answer {
    let cookie = signed_in_cookie(state).await;
    answer(router(state), upload_request(&cookie, page, name, bytes)).await
}

async fn fetch(state: &AppState, url: &str) -> Answer {
    let request = Request::get(url)
        .header(COOKIE, signed_in_cookie(state).await)
        .body(Body::empty())
        .expect("a request builds");
    answer(router(state), request).await
}

/// `file` on `page`, which must be refused to be a test of anything but the refusal.
async fn attach(fixture: &Fixture, name: &str, bytes: &[u8]) -> Value {
    let attached = upload(&fixture.state, fixture.page, name, bytes).await;
    assert_eq!(
        attached.status,
        StatusCode::CREATED,
        "{:?}",
        attached.json()
    );
    attached.json()["attachment"].clone()
}

fn url_of(attachment: &Value) -> &str {
    attachment["url"].as_str().expect("an address")
}

fn stored_count(files: &Files) -> usize {
    files.lock().expect("the fake's lock").len()
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_member_attaches_an_image_and_it_is_served_to_display() {
    let fixture = fixture().await;

    let attached = upload(&fixture.state, fixture.page, "Rollout%20plan.png", PNG).await;
    assert_eq!(attached.status, StatusCode::CREATED);
    let attachment = &attached.json()["attachment"];
    assert_eq!(attachment["name"], "Rollout plan.png");
    assert_eq!(attachment["sizeBytes"], PNG.len());
    assert_eq!(attachment["isImage"], true);
    assert_eq!(attachment["createdBy"], TEST_PERSON_ID.to_string());
    let id = attachment["id"].as_str().expect("an id");
    let page = fixture.page;
    assert_eq!(
        url_of(attachment),
        format!("/api/v1/pages/{page}/attachments/{id}/Rollout-plan.png"),
        "the address stays plain Markdown, and reads as what it is"
    );
    assert_eq!(attached.json()["page"]["attachments"][0]["id"], id);

    let key = format!("page-attachments/{page}/{id}/Rollout-plan.png");
    let stored = fixture.files.lock().expect("the fake's lock")[&key].clone();
    assert_eq!(stored.bytes, PNG, "the whole file reaches the provider");

    let served = fetch(&fixture.state, url_of(attachment)).await;
    assert_eq!(served.status, StatusCode::OK);
    assert_eq!(served.bytes, PNG);
    assert_eq!(served.header(CONTENT_TYPE), "image/png");
    assert_eq!(served.header(X_CONTENT_TYPE_OPTIONS), "nosniff");
    assert!(served.header(CONTENT_DISPOSITION).starts_with("inline; "));
    assert_eq!(served.header(CACHE_CONTROL), "private, max-age=3600");
}

/// The name on the end of an address is for reading, so a page that says it wrong still shows it.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn the_id_finds_a_file_whatever_its_address_is_called() {
    let fixture = fixture().await;
    let attachment = attach(&fixture, "plan.png", PNG).await;
    let page = fixture.page;
    let id = attachment["id"].as_str().expect("an id");

    let served = fetch(
        &fixture.state,
        &format!("/api/v1/pages/{page}/attachments/{id}/anything.html"),
    )
    .await;

    assert_eq!(served.status, StatusCode::OK);
    assert_eq!(served.header(CONTENT_TYPE), "image/png");
}

/// Whoever can upload chooses every byte of a file served on Elysium's own origin.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn active_content_never_runs_on_elysiums_origin() {
    let fixture = fixture().await;
    let script = b"<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>";

    let attachment = attach(&fixture, "diagram.svg", script).await;
    assert_eq!(
        attachment["isImage"], false,
        "it is linked, never displayed"
    );

    let served = fetch(&fixture.state, url_of(&attachment)).await;
    assert_eq!(served.status, StatusCode::OK);
    assert_eq!(served.header(CONTENT_TYPE), "application/octet-stream");
    assert!(
        served
            .header(CONTENT_DISPOSITION)
            .starts_with("attachment; "),
        "{}",
        served.header(CONTENT_DISPOSITION)
    );
    assert_eq!(served.header(X_CONTENT_TYPE_OPTIONS), "nosniff");
    assert!(served.header(CONTENT_SECURITY_POLICY).contains("sandbox"));

    // A text file holding a page's worth of HTML is text to anyone who opens it.
    let attachment = attach(&fixture, "notes.txt", b"<script>alert(1)</script>").await;
    let served = fetch(&fixture.state, url_of(&attachment)).await;
    assert_eq!(served.header(CONTENT_TYPE), "text/plain; charset=utf-8");
    assert!(
        served
            .header(CONTENT_DISPOSITION)
            .starts_with("attachment; ")
    );

    let stored = stored_count(&fixture.files);
    let refused = upload(&fixture.state, fixture.page, "page.html", b"<script>").await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert_eq!(refused.json()["code"], "attachment_type");
    assert_eq!(stored_count(&fixture.files), stored, "nothing was written");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_file_named_as_an_image_must_be_one() {
    let fixture = fixture().await;

    let refused = upload(
        &fixture.state,
        fixture.page,
        "photo.png",
        b"<html><script>alert(1)</script></html>",
    )
    .await;

    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert_eq!(refused.json()["code"], "attachment_not_an_image");
    assert_eq!(stored_count(&fixture.files), 0);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_file_over_the_limit_or_with_nothing_in_it_is_refused_before_it_moves() {
    let fixture = fixture().await;
    let cookie = signed_in_cookie(&fixture.state).await;

    // Its length is all it takes: the file itself is never read.
    let too_large = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/api/v1/pages/{}/attachments?name=big.pdf",
            fixture.page
        ))
        .header(COOKIE, &cookie)
        .header(ORIGIN, "http://localhost:4000")
        .header(CONTENT_LENGTH, MAX_ATTACHMENT_BYTES + 1)
        .body(Body::empty())
        .expect("a request builds");
    let refused = answer(router(&fixture.state), too_large).await;
    assert_eq!(refused.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(refused.json()["code"], "attachment_too_large");

    let refused = upload(&fixture.state, fixture.page, "empty.txt", b"").await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert_eq!(refused.json()["code"], "attachment_empty");

    let unmeasured = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/api/v1/pages/{}/attachments?name=a.txt",
            fixture.page
        ))
        .header(COOKIE, &cookie)
        .header(ORIGIN, "http://localhost:4000")
        .body(Body::from_stream(stream::iter([Ok::<_, std::io::Error>(
            Bytes::from_static(b"hello"),
        )])))
        .expect("a request builds");
    let refused = answer(router(&fixture.state), unmeasured).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);

    assert_eq!(stored_count(&fixture.files), 0);
}

/// A file that fits arrives whole however it is split, and its head is what the check reads.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_file_reaches_the_provider_whole_however_it_arrives() {
    let fixture = fixture().await;
    let cookie = signed_in_cookie(&fixture.state).await;
    let chunks: Vec<Result<Bytes, std::io::Error>> = PNG
        .chunks(3)
        .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
        .collect();
    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/api/v1/pages/{}/attachments?name=a.png",
            fixture.page
        ))
        .header(COOKIE, &cookie)
        .header(ORIGIN, "http://localhost:4000")
        .header(CONTENT_LENGTH, PNG.len())
        .body(Body::from_stream(stream::iter(chunks)))
        .expect("a request builds");

    let attached = answer(router(&fixture.state), request).await;

    assert_eq!(attached.status, StatusCode::CREATED);
    let stored = fixture.files.lock().expect("the fake's lock");
    let file = stored.values().next().expect("a file");
    assert_eq!(file.bytes, PNG);
}

/// Told before anyone waits for a file to go nowhere, and told who can fix it and where.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn with_no_location_chosen_a_file_is_refused_saying_who_can_fix_it() {
    let mut fixture = fixture().await;
    choose(&mut fixture.connection, None).await;

    let refused = upload(&fixture.state, fixture.page, "plan.png", PNG).await;

    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert_eq!(refused.json()["code"], "attachment_no_location");
    let message = refused.json()["message"]
        .as_str()
        .expect("a message")
        .to_owned();
    assert!(
        message.contains("admin") && message.contains("Settings, Storage"),
        "{message}"
    );

    let (status, policy) = call(
        &fixture.state,
        Method::GET,
        "/api/v1/page-attachments",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(policy["policy"]["destinationId"], Value::Null);
    assert_eq!(policy["policy"]["maxBytes"], MAX_ATTACHMENT_BYTES);
    assert!(
        policy["policy"]["extensions"]
            .as_array()
            .expect("extensions")
            .contains(&json!("png"))
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn guests_reach_no_attachment() {
    let mut fixture = fixture().await;
    let attachment = attach(&fixture, "plan.png", PNG).await;
    let id = attachment["id"].as_str().expect("an id").to_owned();
    let page = fixture.page;
    set_role(&mut fixture.connection, "guest").await;

    let refused = upload(&fixture.state, page, "other.png", PNG).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(refused.json()["code"], "members_only");

    let served = fetch(&fixture.state, url_of(&attachment)).await;
    assert_eq!(served.status, StatusCode::FORBIDDEN);
    assert_eq!(served.json()["code"], "members_only");

    for (method, path) in [
        (
            Method::DELETE,
            format!("/api/v1/pages/{page}/attachments/{id}"),
        ),
        (
            Method::POST,
            format!("/api/v1/pages/{page}/attachments/{id}/restore"),
        ),
        (Method::GET, "/api/v1/page-attachments".to_owned()),
    ] {
        let (status, answer) = call(&fixture.state, method, &path, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {answer}");
        assert_eq!(answer["code"], "members_only", "{path}");
    }
}

/// An attachment is not content: it makes no version, and an unchanged page stays unchanged.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn attaching_makes_no_version_and_records_who_did_it() {
    let mut fixture = fixture().await;
    let before = page::find(&mut fixture.connection, fixture.page)
        .await
        .expect("found");

    attach(&fixture, "plan.png", PNG).await;

    let after = page::find(&mut fixture.connection, fixture.page)
        .await
        .expect("found");
    assert_eq!(after.current_version, before.current_version);
    assert_eq!(after.body, before.body);
    assert_eq!(after.updated_at, before.updated_at);

    let history = page::history(&mut fixture.connection, fixture.page)
        .await
        .expect("read");
    assert_eq!(kinds(&history), ["created", "attached"]);
    assert_eq!(history[1].data["name"], "plan.png");
    assert_eq!(history[1].created_by, TEST_PERSON_ID);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn removing_a_file_closes_its_address_and_putting_it_back_opens_it() {
    let mut fixture = fixture().await;
    let attachment = attach(&fixture, "plan.png", PNG).await;
    let id = attachment["id"].as_str().expect("an id").to_owned();
    let page = fixture.page;
    let path = format!("/api/v1/pages/{page}/attachments/{id}");

    let (status, removed) = call(&fixture.state, Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::OK, "{removed}");
    assert_eq!(removed["page"]["attachments"], json!([]));
    assert_eq!(
        fetch(&fixture.state, url_of(&attachment)).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        stored_count(&fixture.files),
        1,
        "the file stays, so it can be put back"
    );

    let (status, _) = call(&fixture.state, Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::OK, "removing it twice records nothing");

    let (status, restored) = call(
        &fixture.state,
        Method::POST,
        &format!("{path}/restore"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["page"]["attachments"][0]["id"], id.as_str());
    assert_eq!(fetch(&fixture.state, url_of(&attachment)).await.bytes, PNG);

    let history = page::history(&mut fixture.connection, page)
        .await
        .expect("read");
    assert_eq!(
        kinds(&history),
        ["created", "attached", "detached", "attached"]
    );

    let (status, _) = call(
        &fixture.state,
        Method::DELETE,
        &format!("/api/v1/pages/{page}/attachments/{}", Uuid::now_v7()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// A deleted page reads on at its address, its files with it, and changes no further.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_deleted_page_keeps_its_files_and_takes_no_more() {
    let mut fixture = fixture().await;
    let attachment = attach(&fixture, "plan.png", PNG).await;
    let id = attachment["id"].as_str().expect("an id").to_owned();
    let page_id = fixture.page;
    page::delete(&mut fixture.connection, page_id, author(None), None)
        .await
        .expect("deleted");

    let served = fetch(&fixture.state, url_of(&attachment)).await;
    assert_eq!(served.status, StatusCode::OK);
    assert_eq!(served.bytes, PNG);

    let (status, read) = call(
        &fixture.state,
        Method::GET,
        &format!("/api/v1/pages/{page_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read["page"]["attachments"][0]["id"], id.as_str());

    let refused = upload(&fixture.state, page_id, "other.png", PNG).await;
    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert_eq!(refused.json()["code"], "page_deleted");

    let (status, refused) = call(
        &fixture.state,
        Method::DELETE,
        &format!("/api/v1/pages/{page_id}/attachments/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["code"], "page_deleted");

    page::restore(&mut fixture.connection, page_id, author(None), None)
        .await
        .expect("restored");
    let served = fetch(&fixture.state, url_of(&attachment)).await;
    assert_eq!(
        served.status,
        StatusCode::OK,
        "it comes back with its files"
    );
    assert_eq!(stored_count(&fixture.files), 1);
}

/// A page deleted while its file is on the way leaves nothing behind that no row names.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_file_refused_after_it_was_written_is_deleted_again() {
    let fixture = fixture().await;
    let state = fixture.state.clone();
    let page_id = fixture.page;
    let deleting = state.clone();

    // The page goes while the file's first bytes are read, once every earlier check has passed.
    let chunks = stream::once(async move {
        let mut connection = deleting.database.get().await.expect("a connection");
        page::delete(&mut connection, page_id, author(None), None)
            .await
            .expect("deleted");
        Ok::<_, std::io::Error>(Bytes::from_static(PNG))
    });
    let refused = upload_attachment::upload_file(
        &state,
        author(None),
        page_id,
        Upload {
            name: "plan.png".to_owned(),
            size_bytes: PNG.len() as u64,
            chunks,
        },
    )
    .await
    .expect_err("the page is gone");

    let (status, _) = refused.into_status_and_body();
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(stored_count(&fixture.files), 0);
}

/// A file shows on its page, so a proposal-only page keeps it from agents as it keeps its text, and
/// a fixed page takes files as it takes edits.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn agents_attach_where_they_may_write() {
    let mut fixture = fixture().await;
    let session = Agent::session(12);
    let doc = Section::Glossary.page_id();
    let elysia = Author {
        user_id: ELYSIA_USER_ID,
        agent: None,
        at: chrono::Utc::now(),
    };

    for author in [author(Some(&session)), elysia] {
        let refused = page_attachment::destination_for(&mut fixture.connection, doc, author).await;
        assert!(
            matches!(refused, Err(PageError::ProposalOnly)),
            "{refused:?}"
        );
    }
    page_attachment::destination_for(&mut fixture.connection, doc, author(None))
        .await
        .expect("a person edits it directly");
    page_attachment::destination_for(
        &mut fixture.connection,
        LEARNINGS_PAGE_ID,
        author(Some(&session)),
    )
    .await
    .expect("a fixed page keeps its place, not its text");
    page_attachment::destination_for(
        &mut fixture.connection,
        fixture.page,
        author(Some(&session)),
    )
    .await
    .expect("an agent writes an ordinary page");
}

/// What the file was written by stays on it, so a page says when an agent added something.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn an_attachment_names_the_agent_that_added_it() {
    let mut fixture = fixture().await;
    let agent = Agent::mcp_client("claude-code-client", "Claude Code").expect("recordable");
    let id = Uuid::now_v7();

    let (attachment, _) = page_attachment::add(
        &mut fixture.connection,
        stored_file(fixture.page, fixture.location.id, id),
        author(Some(&agent)),
    )
    .await
    .expect("added");

    assert_eq!(attachment.agent.as_deref(), Some(agent.id()));
    assert_eq!(attachment.agent_name.as_deref(), Some("Claude Code"));
}

#[derive(QueryableByName)]
struct AuditEntry {
    #[diesel(sql_type = Jsonb)]
    data: Value,
}

fn stored_file(page_id: Uuid, storage_location_id: Uuid, id: Uuid) -> StoredFile<'static> {
    StoredFile {
        id,
        page_id,
        storage_location_id,
        storage_path: "page-attachments/anything",
        file_name: "plan.pdf",
        content_type: "application/pdf",
        size_bytes: 1,
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_page_holds_a_limited_number_of_files() {
    let mut fixture = fixture().await;
    let held = usize::try_from(MAX_ATTACHMENTS_PER_PAGE).expect("a small limit");
    let page = fixture.page;
    let location = fixture.location.id;
    for _ in 0..held {
        page_attachment::add(
            &mut fixture.connection,
            stored_file(page, location, Uuid::now_v7()),
            author(None),
        )
        .await
        .expect("added");
    }

    let refused = upload(&fixture.state, page, "one-more.png", PNG).await;

    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert_eq!(refused.json()["code"], "page_attachment_limit");
}

/// A learning's history is what it said, and files are its page's alone, as moves and labels are.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_learnings_history_leaves_its_files_out() {
    let mut fixture = fixture().await;
    let saved = learning::save(
        &mut fixture.connection,
        LearningContent {
            title: "Restart nginx after a config change".to_owned(),
            body: "A reload keeps the old upstreams.".to_owned(),
            category: "decision".to_owned(),
            evidence: "Seen on the VM.".to_owned(),
        },
        author(None),
    )
    .await
    .expect("saved");
    let id = saved.learning.id;
    page_attachment::add(
        &mut fixture.connection,
        stored_file(id, fixture.location.id, Uuid::now_v7()),
        author(None),
    )
    .await
    .expect("added");

    let history = learning::history(&mut fixture.connection, id)
        .await
        .expect("read");

    assert_eq!(history.len(), 1);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn only_an_admin_chooses_where_attachments_go_and_the_change_is_audited() {
    let mut fixture = fixture().await;
    let path = "/api/v1/page-attachments/destination";
    let other =
        location_in_the_fake_zone(&mut fixture.connection, &fixture.state.cipher, "Other").await;

    let body = json!({ "storageLocationId": other.id });
    set_role(&mut fixture.connection, "member").await;
    let (status, refused) = call(&fixture.state, Method::PUT, path, Some(&body)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");

    set_role(&mut fixture.connection, "admin").await;
    let (status, chosen) = call(&fixture.state, Method::PUT, path, Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{chosen}");
    assert_eq!(chosen["policy"]["destinationId"], other.id.to_string());

    let (status, refused) = call(&fixture.state, Method::PUT, path, Some(&json!({}))).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a body missing it never turns attachments off"
    );
    assert!(
        refused["message"]
            .as_str()
            .expect("a message")
            .contains("required")
    );

    let body = json!({ "storageLocationId": Uuid::now_v7() });
    let (status, _) = call(&fixture.state, Method::PUT, path, Some(&body)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let audited: AuditEntry = diesel::sql_query(
        "SELECT data FROM user_events WHERE kind = 'workspace_settings_changed' \
         ORDER BY id DESC LIMIT 1",
    )
    .get_result(&mut fixture.connection)
    .await
    .expect("the change is recorded");
    assert_eq!(
        audited.data["after"]["pageAttachmentStorageLocationId"],
        other.id.to_string()
    );
}

/// A file stays where it landed, whichever location is chosen later, and holds that location.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_location_stays_while_it_is_chosen_or_holds_a_file() {
    let mut fixture = fixture().await;
    let attachment = attach(&fixture, "plan.png", PNG).await;
    let holding = format!("/api/v1/storage-locations/{}", fixture.location.id);

    let other =
        location_in_the_fake_zone(&mut fixture.connection, &fixture.state.cipher, "Other").await;
    choose(&mut fixture.connection, Some(other.id)).await;

    let (status, refused) = call(&fixture.state, Method::DELETE, &holding, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["code"], "attachment_files");

    let chosen = format!("/api/v1/storage-locations/{}", other.id);
    let (status, refused) = call(&fixture.state, Method::DELETE, &chosen, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["code"], "attachment_destination");

    assert_eq!(fetch(&fixture.state, url_of(&attachment)).await.bytes, PNG);
}

/// The one route with a deadline of its own is named by its pattern, so this is what ties the two
/// together: a deadline nothing could meet still lets a file through, and nothing else.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn the_upload_route_is_the_one_the_request_deadline_leaves_out() {
    let fixture = fixture().await;
    let cookie = signed_in_cookie(&fixture.state).await;
    let timed = || {
        routes::router(&fixture.state)
            .with_state(fixture.state.clone())
            .layer(from_fn_with_state(Duration::ZERO, request_timeout::enforce))
    };

    let attached = answer(
        timed(),
        upload_request(&cookie, fixture.page, "plan.png", PNG),
    )
    .await;
    assert_eq!(attached.status, StatusCode::CREATED);

    let request = Request::get("/api/v1/pages")
        .header(COOKIE, &cookie)
        .body(Body::empty())
        .expect("a request builds");
    let listed = answer(timed(), request).await;
    assert_eq!(listed.status, StatusCode::REQUEST_TIMEOUT);
}
