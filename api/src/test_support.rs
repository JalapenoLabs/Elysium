// Copyright © 2026 Jalapeno Labs

//! Helpers for tests that need a real Postgres.
//!
//! Those tests are `#[ignore]`d so `cargo test` stays hermetic. Run them with
//! `scripts/verify-migrations.sh`, which starts a disposable Postgres and sets
//! `TEST_DATABASE_URL`. Each test creates its own database, so they run in
//! parallel without seeing each other's schema or rows.

use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use secrecy::{ExposeSecret, SecretString};
use uuid::{Uuid, uuid};

use crate::action_items::Actor;
use crate::crypto::{Cipher, generate_key};
use crate::database::migrations::{self, MigrationCommand};

/// The person tests act as. [`migrated_database`] creates their account, so rows naming
/// them as creator satisfy `created_by`'s foreign key.
pub const TEST_PERSON_ID: Uuid = uuid!("0199a3c4-0000-7000-8000-00000000beef");

/// [`TEST_PERSON_ID`] as the actor history records.
pub const TEST_PERSON: Actor = Actor::User(TEST_PERSON_ID);

/// The Kratos identity [`TEST_PERSON_ID`] signs in as. Fixed, so [`signed_in_cookie`] can hand
/// the auth layer a session that maps to the test person's row.
pub const TEST_PERSON_KRATOS_ID: Uuid = uuid!("0199a3c4-0000-7000-8000-00000000cafe");

/// Creates an empty database and returns its URL. Nothing drops it: the Postgres
/// these tests run against is thrown away when the script exits.
///
/// # Panics
/// Panics when `TEST_DATABASE_URL` is unset or the server refuses the connection,
/// since the test cannot say anything meaningful without a database.
pub async fn empty_database() -> SecretString {
    let admin_url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must point at a disposable Postgres");
    let mut admin = AsyncPgConnection::establish(&admin_url)
        .await
        .expect("TEST_DATABASE_URL accepts connections");

    let name = format!("elysium_test_{}", Uuid::now_v7().simple());
    diesel::sql_query(format!("CREATE DATABASE {name}"))
        .execute(&mut admin)
        .await
        .expect("the test role may create databases");

    let (server, _database) = admin_url
        .rsplit_once('/')
        .expect("TEST_DATABASE_URL ends in /<database>");
    SecretString::from(format!("{server}/{name}"))
}

/// An empty database with every migration applied, plus a connection to it.
pub async fn migrated_database() -> (SecretString, AsyncPgConnection) {
    let url = empty_database().await;
    migrations::execute(url.clone(), MigrationCommand::Run)
        .await
        .expect("migrations apply to an empty database");
    let mut connection = AsyncPgConnection::establish(url.expose_secret())
        .await
        .expect("migrated database accepts connections");
    diesel::sql_query(
        "INSERT INTO users (id, kratos_identity_id, email, name, role, status, approved_at) \
         VALUES ($1, $2, 'tester@example.com', 'Tester', 'admin', 'active', now())",
    )
    .bind::<diesel::sql_types::Uuid, _>(TEST_PERSON_ID)
    .bind::<diesel::sql_types::Uuid, _>(TEST_PERSON_KRATOS_ID)
    .execute(&mut connection)
    .await
    .expect("the test person is created");
    (url, connection)
}

/// A cipher under a fresh random key.
pub fn cipher() -> Cipher {
    Cipher::from_base64_key(&SecretString::from(generate_key())).expect("generated keys are valid")
}

/// A Bunny location for every project, for tests that need somewhere to keep files but never
/// reach the provider.
pub fn location_for_every_project(
    name: &str,
) -> crate::models::storage_location::NewStorageLocation {
    use crate::models::project::ProjectScope;
    use crate::models::storage_location::{
        BunnyStorageRegion, NewStorageLocation, StorageProvider,
    };

    NewStorageLocation {
        created_by: crate::test_support::TEST_PERSON_ID,
        name: name.to_owned(),
        provider: StorageProvider::Bunny {
            zone: "elysium-files".to_owned(),
            region: BunnyStorageRegion::NewYork,
        },
        path_prefix: String::new(),
        storage_limit_bytes: None,
        access_key: SecretString::from(format!("zone-password-{name}")),
        projects: ProjectScope::All,
    }
}

/// A whole [`AppState`] against `database_url` and the Redis at `TEST_REDIS_URL`, for tests that
/// drive the router itself. Kratos, satellites, and mail point nowhere reachable: a test that
/// needs them fakes them, and one that keeps files uses [`app_state_with_storage`].
///
/// # Panics
/// Panics when `TEST_REDIS_URL` is unset or Redis refuses the connection.
pub async fn app_state(database_url: &SecretString) -> crate::state::AppState {
    let storage = crate::storage::Storage::new(reqwest::Client::new());
    app_state_with_storage(database_url, storage).await
}

/// [`app_state`] keeping files through `storage`, such as the storage module's fake Bunny
/// zone. The fleet is built with it too, so the files its watchers keep land there as well.
///
/// # Panics
/// Panics when `TEST_REDIS_URL` is unset or Redis refuses the connection.
pub async fn app_state_with_storage(
    database_url: &SecretString,
    storage: crate::storage::Storage,
) -> crate::state::AppState {
    use std::sync::Arc;

    use url::Url;

    use crate::action_items::links::Links;
    use crate::config::{Config, MailConfig};
    use crate::fleet::Fleet;
    use crate::github::Github;
    use crate::jira::Jira;
    use crate::realtime::EventBus;
    use crate::version::VersionInfo;

    let redis_url = SecretString::from(
        std::env::var("TEST_REDIS_URL").expect("TEST_REDIS_URL must point at a disposable Redis"),
    );
    let nowhere = Url::parse("http://127.0.0.1:9/").expect("a static URL parses");
    let config = Config {
        bind_address: "127.0.0.1:0".parse().expect("a static address parses"),
        database_url: database_url.clone(),
        redis_url: redis_url.clone(),
        encryption_key: SecretString::from(generate_key()),
        public_url: Url::parse("http://localhost:4000").expect("a static URL parses"),
        kratos_public_url: nowhere.clone(),
        kratos_admin_url: nowhere.clone(),
        hydra_admin_url: nowhere.clone(),
        database_max_connections: 4,
        cors_allowed_origins: Vec::new(),
        request_timeout: std::time::Duration::from_secs(5),
        max_request_body_bytes: 1 << 20,
        frontend_dir: None,
        mail: MailConfig {
            oauth_broker: None,
            oauth_broker_internal: None,
            docker: nowhere,
            ingress_address: None,
        },
    };

    let database = crate::connections::connect_postgres(database_url, 4)
        .await
        .expect("Postgres accepts connections");
    let redis = crate::connections::connect_redis(&redis_url)
        .await
        .expect("TEST_REDIS_URL accepts connections");
    let cipher = Arc::new(cipher());
    let http = reqwest::Client::new();
    let events = EventBus::new();
    let shutdown = tokio_util::sync::CancellationToken::new();
    let github = Github::new(http.clone());
    let jira = Jira::new(http.clone());
    let links = Links::new(database.clone(), Arc::clone(&cipher), &jira, &github);
    let fleet = Fleet::new(
        database.clone(),
        Arc::clone(&cipher),
        events.clone(),
        storage.clone(),
        links.clone(),
        shutdown.clone(),
    );
    let mail = crate::server::build_mail(
        &config,
        http.clone(),
        database.clone(),
        Arc::clone(&cipher),
        events.clone(),
    )
    .expect("mail services build without reaching anything");

    crate::state::AppState {
        auth: crate::server::build_auth(&config, http),
        database,
        redis,
        cipher,
        version: Arc::new(VersionInfo::from_build().expect("build metadata parses")),
        events,
        fleet,
        github,
        jira,
        links,
        mail,
        storage,
        shutdown,
    }
}

/// Signs [`TEST_PERSON_ID`] in for a router test, and returns the `Cookie` header to send.
///
/// The auth layer asks Redis before Kratos, so caching a session as Kratos would answer it is
/// all it takes. The session is a fresh one per call, signed in with two factors so the
/// workspace's authenticator rule never applies, and far enough from expiry that nothing asks
/// Kratos to extend it.
pub async fn signed_in_cookie(state: &crate::state::AppState) -> String {
    use crate::auth::kratos::{AssuranceLevel, Identity, SESSION_COOKIE, Session, Traits};

    let token = format!("ory_st_test_{}", Uuid::now_v7().simple());
    let session = Session {
        id: Uuid::now_v7(),
        expires_at: chrono::Utc::now() + chrono::Duration::hours(72),
        authenticator_assurance_level: AssuranceLevel::Aal2,
        identity: Identity {
            id: TEST_PERSON_KRATOS_ID,
            traits: Traits {
                email: "tester@example.com".to_owned(),
                name: "Tester".to_owned(),
            },
        },
    };
    crate::auth::sessions::remember(&state.redis, &token, &session).await;
    format!("{SESSION_COOKIE}={token}")
}

/// The boundary [`multipart_form`] separates parts with.
const MULTIPART_BOUNDARY: &str = "elysium-test-boundary";

/// A `multipart/form-data` body of `parts`, each a field name and its content, and the
/// `Content-Type` header that goes with it.
pub fn multipart_form(parts: &[(&str, &[u8])]) -> (String, Vec<u8>) {
    let mut body = Vec::new();
    for (name, content) in parts {
        let disposition = format!(
            "--{MULTIPART_BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n"
        );
        body.extend_from_slice(disposition.as_bytes());
        body.extend_from_slice(content);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{MULTIPART_BOUNDARY}--\r\n").as_bytes());
    let content_type = format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}");
    (content_type, body)
}

/// What [`studio_item_with_session`] creates.
#[derive(Debug)]
pub struct StudioFixture {
    pub satellite_id: Uuid,
    pub item: crate::models::studio_item::StudioItem,
    /// The item's one session, on the satellite's [`THREAD_ID`](crate::fleet::fake_satellite::THREAD_ID).
    pub session: crate::models::coding_session::CodingSession,
}

/// A Studio item named "Banana", first asked to "Model a banana", whose files go to the storage
/// module's fake Bunny zone, with one session on a satellite at `satellite_url`.
///
/// Secrets are sealed with `cipher`, which must be the one the code under test opens them with.
pub async fn studio_item_with_session(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    satellite_url: &str,
) -> StudioFixture {
    use crate::fleet::fake_satellite::THREAD_ID;
    use crate::models::coding_session::{self, NewCodingSession};
    use crate::models::project::ProjectScope;
    use crate::models::satellite::{self, NewSatellite};
    use crate::models::storage_location::{
        self, BunnyStorageRegion, NewStorageLocation, StorageProvider,
    };
    use crate::models::studio_item::{self, NewStudioItem};
    use crate::storage::tests::PASSWORD;

    let location = storage_location::create(
        connection,
        cipher,
        &NewStorageLocation {
            created_by: TEST_PERSON_ID,
            name: "Studio files".to_owned(),
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
    .expect("location");
    let item = studio_item::create(
        connection,
        &NewStudioItem {
            created_by: TEST_PERSON_ID,
            id: Uuid::now_v7(),
            title: "Banana".to_owned(),
            prompt: "Model a banana".to_owned(),
            project_id: None,
            storage_location_id: location.id,
        },
    )
    .await
    .expect("item");
    let satellite_id = satellite::create(
        connection,
        cipher,
        &NewSatellite {
            created_by: TEST_PERSON_ID,
            name: "orbit".to_owned(),
            description: String::new(),
            url: satellite_url.to_owned(),
            secret: SecretString::from("satellite-secret"),
            is_active: true,
        },
    )
    .await
    .expect("satellite")
    .id;
    let session_id = coding_session::reserve_id(connection)
        .await
        .expect("reserve");
    let session = coding_session::create(
        connection,
        &NewCodingSession {
            created_by: TEST_PERSON_ID,
            id: session_id,
            project_id: None,
            satellite_id,
            thread_id: THREAD_ID.to_owned(),
            title: "Banana".to_owned(),
            github_credential_id: None,
            action_item_id: None,
            studio_item_id: Some(item.id),
        },
    )
    .await
    .expect("session");
    StudioFixture {
        satellite_id,
        item,
        session,
    }
}
