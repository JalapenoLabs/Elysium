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

/// Names a public configuration repository and records a good fetch of it, so workers and
/// Project Managers may start (`crate::agent_configuration`), as on any deployment set up for
/// them. Tests of the hold itself leave this out.
pub async fn fetched_agent_configuration(connection: &mut AsyncPgConnection) {
    use diesel::ExpressionMethods;

    use crate::database::schema::workspace_settings;
    use crate::runner::configuration::{Checkout, Source};

    let repository = "https://github.com/acme/agents".to_owned();
    diesel::update(workspace_settings::table)
        .set(workspace_settings::agent_configuration_repository.eq(&repository))
        .execute(connection)
        .await
        .expect("the repository is named");
    let source = Source {
        repository: repository.clone(),
        github_credential_id: None,
    };
    let checkout = Checkout {
        repository,
        commit: "611c46a583443c3a8dd4caad4a89dffa964add43".to_owned(),
        subject: "Agents' configuration".to_owned(),
        fetched_at: chrono::Utc::now(),
        skipped: Vec::new(),
    };
    crate::models::agent_configuration::record_fetched(connection, &source, &checkout)
        .await
        .expect("the fetch is recorded");
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
/// drive the router itself. Kratos, Docker, and mail point nowhere reachable: a test that
/// needs them fakes them, and one that keeps files uses [`app_state_with_storage`].
///
/// # Panics
/// Panics when `TEST_REDIS_URL` is unset or Redis refuses the connection.
pub async fn app_state(database_url: &SecretString) -> crate::state::AppState {
    let storage = crate::storage::Storage::new(reqwest::Client::new());
    app_state_with_storage(database_url, storage).await
}

/// [`app_state`] keeping files through `storage`, such as the storage module's fake Bunny
/// zone. It is built exactly as the API builds its own (`crate::services::build_state`), on
/// connections of the test's own.
///
/// # Panics
/// Panics when `TEST_REDIS_URL` is unset or Redis refuses the connection.
pub async fn app_state_with_storage(
    database_url: &SecretString,
    storage: crate::storage::Storage,
) -> crate::state::AppState {
    use std::sync::Arc;

    use url::Url;

    use crate::config::{Config, MailConfig};
    use crate::realtime::{EVENTS_CHANNEL, EventBus};
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
        docker: nowhere.clone(),
        workspace_image: "elysium-workspace:test".to_owned(),
        whisper_image: "elysium-whisper:test".to_owned(),
        embeddings: nowhere,
        mail: MailConfig {
            oauth_broker: None,
            oauth_broker_internal: None,
            ingress_address: None,
        },
    };

    let database = crate::connections::connect_postgres(database_url, 4)
        .await
        .expect("Postgres accepts connections");
    let redis = crate::connections::connect_redis(&redis_url)
        .await
        .expect("TEST_REDIS_URL accepts connections");
    // Through Redis, as in production, on a channel of this test's own: pub/sub channels span
    // the whole server, and every test shares it.
    let channel = format!("{EVENTS_CHANNEL}:test:{}", Uuid::now_v7().simple());
    let events = EventBus::connect_on(&redis_url, &channel)
        .await
        .expect("TEST_REDIS_URL accepts a subscription");
    let mut foundation = crate::services::Foundation::assemble(
        config,
        Arc::new(VersionInfo::from_build().expect("build metadata parses")),
        Arc::new(cipher()),
        database,
        redis,
        reqwest::Client::new(),
        events,
    );
    foundation.storage = storage;
    // Nothing in it starts a loop, so no test reaches Docker, Kratos, or mail through it; the
    // vault's requests go unanswered, since no engine runs.
    let (state, _vault_replies) =
        crate::server::build_state(&foundation).expect("the API's services build");
    state
}

/// Holds time still for `state`'s budgets per person, the global search's and the knowledge
/// search's, so a test can count one out to the last request.
///
/// On a busy runner each request through the router can take long enough that a real bucket
/// refills about as fast as the test spends it, and the refusal it counts on never comes.
/// Every budget starts afresh.
pub fn hold_budgets_still(state: &mut crate::state::AppState) {
    use std::sync::Arc;

    use crate::middleware::rate_limit::{HeldClock, Limiter, LimiterClock};

    let clock = LimiterClock::Held(HeldClock::new());
    state.search_limiter = Arc::new(Limiter::with_clock(
        clock.clone(),
        crate::search::PER_PERSON,
    ));
    state.knowledge = state.knowledge.clone().with_budget_clock(clock);
}

/// Signs [`TEST_PERSON_ID`] in for a router test, and returns the `Cookie` header to send.
///
/// The auth layer asks Redis before Kratos, so caching a session as Kratos would answer it is
/// all it takes. The session is a fresh one per call, signed in with two factors so the
/// workspace's authenticator rule never applies, and far enough from expiry that nothing asks
/// Kratos to extend it.
pub async fn signed_in_cookie(state: &crate::state::AppState) -> String {
    use crate::auth::kratos::{
        AssuranceLevel, Identity, PersonName, SESSION_COOKIE, Session, Traits,
    };

    let token = format!("ory_st_test_{}", Uuid::now_v7().simple());
    let session = Session {
        id: Uuid::now_v7(),
        expires_at: chrono::Utc::now() + chrono::Duration::hours(72),
        authenticated_at: chrono::Utc::now(),
        authenticator_assurance_level: AssuranceLevel::Aal2,
        identity: Identity {
            id: TEST_PERSON_KRATOS_ID,
            traits: Traits {
                email: "tester@example.com".to_owned(),
                name: PersonName::Split {
                    first: "Tester".to_owned(),
                    last: String::new(),
                },
            },
        },
    };
    crate::auth::sessions::remember(&state.redis, &token, &session).await;
    format!("{SESSION_COOKIE}={token}")
}

/// An MCP access token for [`TEST_PERSON_ID`], granting `scopes`, as the `Authorization` header
/// value to send to `/api/mcp`.
///
/// The bearer check asks Redis before Hydra, so caching Hydra's answer as it would give it is all
/// it takes, as [`signed_in_cookie`] does for a browser session. Each call is a fresh token.
pub async fn mcp_bearer(state: &crate::state::AppState, scopes: &[&str]) -> String {
    mcp_bearer_of_client(state, scopes, "test-client").await
}

/// As [`mcp_bearer`], for the OAuth client `client_id`.
pub async fn mcp_bearer_of_client(
    state: &crate::state::AppState,
    scopes: &[&str],
    client_id: &str,
) -> String {
    use crate::oauth::hydra::Introspection;
    use crate::oauth::{mcp_resource, tokens};

    let token = format!("ory_at_test_{}", Uuid::now_v7().simple());
    let introspection = Introspection {
        active: true,
        sub: Some(TEST_PERSON_ID.to_string()),
        scope: Some(scopes.join(" ")),
        aud: vec![mcp_resource(&state.auth.public_url)],
        client_id: Some(client_id.to_owned()),
        exp: Some(chrono::Utc::now().timestamp() + 3600),
        token_use: Some("access_token".to_owned()),
    };
    tokens::remember(&state.redis, &token, &introspection).await;
    format!("Bearer {token}")
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

/// A Studio item named "Banana", first asked to "Model a banana", whose files go to the storage
/// module's fake Bunny zone.
///
/// Secrets are sealed with `cipher`, which must be the one the code under test opens them with.
pub async fn studio_item(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
) -> crate::models::studio_item::StudioItem {
    use crate::models::project::ProjectScope;
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
    studio_item::create(
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
    .expect("item")
}
