// Copyright © 2026 Jalapeno Labs

//! The vault against a fake CLI: a shell script standing in for `bw`'s one-shot commands, and
//! an HTTP server standing in for `bw serve`, which answers every item whatever the filter, so
//! the tests prove Elysium's own collection check.

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::extract::{Path as UrlPath, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};

use super::*;
use crate::test_support::{TEST_PERSON_ID, migrated_database};

const ELYSIA_COLLECTION: Uuid = Uuid::from_u128(0xe1);
const OTHER_COLLECTION: Uuid = Uuid::from_u128(0x07);
const ORGANIZATION: Uuid = Uuid::from_u128(0x0f);
const PIZZA: Uuid = Uuid::from_u128(0x01);
const BANK: Uuid = Uuid::from_u128(0x02);
const MASTER_PASSWORD: &str = "correct horse battery staple";
const CLIENT_SECRET: &str = "right-client-secret";
const PIZZA_PASSWORD: &str = "pizza-password-123";
const TOTP_CODE: &str = "492039";

/// `bw`'s one-shot commands. It keeps whether it is signed in in a file in its directory, and
/// refuses any client secret but the right one; `serve` only sleeps, since the fake server
/// below answers in its place.
const FAKE_CLI: &str = r#"#!/bin/sh
state="$BITWARDENCLI_APPDATA_DIR/fake-state"
server="$BITWARDENCLI_APPDATA_DIR/fake-server"
case "$1" in
  status)
    status=$(cat "$state" 2>/dev/null || echo unauthenticated)
    url=$(cat "$server" 2>/dev/null || echo https://vault.bitwarden.com)
    printf '{"success":true,"data":{"object":"template","template":{"serverUrl":"%s","userEmail":"elysia@example.com","status":"%s"}}}' "$url" "$status"
    ;;
  config)
    echo "$3" > "$server"
    printf '{"success":true,"data":{"object":"message","title":"Saved setting `config`."}}'
    ;;
  login)
    if [ "$BW_CLIENTSECRET" != "right-client-secret" ] || [ -z "$BW_CLIENTID" ]; then
      printf '{"success":false,"message":"client_id or client_secret is incorrect. Try again."}'
      exit 1
    fi
    echo locked > "$state"
    printf '{"success":true,"data":{"object":"message","title":"You are logged in!"}}'
    ;;
  logout)
    echo unauthenticated > "$state"
    printf '{"success":true,"data":{"object":"message","title":"You have logged out."}}'
    ;;
  serve)
    exec sleep 3600
    ;;
esac
"#;

#[derive(Default)]
struct FakeServe {
    unlocked: AtomicBool,
}

fn item(id: Uuid, name: &str, collection: Uuid, password: &str) -> Value {
    json!({
        "object": "item",
        "id": id,
        "organizationId": ORGANIZATION,
        "type": 1,
        "name": name,
        "login": {
            "uris": [{ "match": null, "uri": format!("https://{}.example.com", name.to_lowercase()) }],
            "username": "alex@example.com",
            "password": password,
            "totp": "JBSWY3DPEHPK3PXP"
        },
        "collectionIds": [collection],
        "deletedDate": null
    })
}

fn items() -> Vec<Value> {
    vec![
        item(PIZZA, "Pizza", ELYSIA_COLLECTION, PIZZA_PASSWORD),
        item(BANK, "Bank", OTHER_COLLECTION, "bank-password-456"),
    ]
}

fn ok(data: Value) -> Json<Value> {
    let mut answer = json!({ "success": true });
    answer["data"] = data;
    Json(answer)
}

fn refused(message: &str) -> Json<Value> {
    Json(json!({ "success": false, "message": message }))
}

async fn start_fake_serve() -> (u16, Arc<FakeServe>) {
    let fake = Arc::new(FakeServe::default());
    let router = Router::new()
        .route(
            "/status",
            get(|State(fake): State<Arc<FakeServe>>| async move {
                let status = if fake.unlocked.load(Ordering::SeqCst) {
                    "unlocked"
                } else {
                    "locked"
                };
                ok(json!({ "object": "template", "template": {
                    "serverUrl": "https://vault.example.com",
                    "userEmail": "elysia@example.com",
                    "status": status
                }}))
            }),
        )
        .route(
            "/unlock",
            post(
                |State(fake): State<Arc<FakeServe>>, Json(body): Json<Value>| async move {
                    if body["password"] != MASTER_PASSWORD {
                        return refused("Invalid master password.");
                    }
                    fake.unlocked.store(true, Ordering::SeqCst);
                    ok(json!({ "object": "message", "raw": "session-key" }))
                },
            ),
        )
        .route(
            "/sync",
            post(|| async { ok(json!({ "object": "message", "title": "Syncing complete." })) }),
        )
        .route(
            "/list/object/collections",
            get(|| async {
                ok(json!({ "object": "list", "data": [
                    { "object": "collection", "id": ELYSIA_COLLECTION, "organizationId": ORGANIZATION, "name": "Elysia" },
                    { "object": "collection", "id": OTHER_COLLECTION, "organizationId": ORGANIZATION, "name": "Finance" }
                ]}))
            }),
        )
        .route(
            "/list/object/items",
            get(|| async { ok(json!({ "object": "list", "data": items() })) }),
        )
        .route(
            "/object/item/{id}",
            get(|UrlPath(id): UrlPath<Uuid>| async move {
                items()
                    .into_iter()
                    .find(|item| item["id"] == json!(id))
                    .map_or_else(|| refused("Not found."), ok)
            }),
        )
        .route(
            "/object/totp/{id}",
            get(|| async { ok(json!({ "object": "string", "data": TOTP_CODE })) }),
        )
        .with_state(Arc::clone(&fake));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let port = listener.local_addr().expect("an address").port();
    tokio::spawn(async move { axum::serve(listener, router).await });
    (port, fake)
}

fn write_fake_cli(directory: &Path) -> PathBuf {
    let binary = directory.join("bw");
    std::fs::write(&binary, FAKE_CLI).expect("the fake CLI is written");
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
        .expect("the fake CLI is executable");
    binary
}

fn credentials(client_secret: &str, master_password: &str) -> Credentials {
    Credentials {
        server: Server::SelfHosted(Url::parse("https://vault.example.com").expect("a URL")),
        client_id: "user.6c2b0d8e-2f9e-4c52-9a8e-0e3c1f6e8d11".to_owned(),
        client_secret: SecretString::from(client_secret),
        master_password: SecretString::from(master_password),
    }
}

async fn passwords(directory: &Path) -> (Passwords, Arc<FakeServe>) {
    let (url, _connection) = migrated_database().await;
    let database = crate::connections::connect_postgres(&url, 2)
        .await
        .expect("Postgres accepts connections");
    let (port, fake) = start_fake_serve().await;
    let passwords = Passwords::new(PasswordsDeps {
        database,
        cipher: Arc::new(crate::test_support::cipher()),
        events: EventBus::new(),
        binary: write_fake_cli(directory),
        directory: directory.join("state"),
        serve_port: port,
    });
    (passwords, fake)
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn agents_read_only_the_chosen_collection() {
    let directory = tempfile::tempdir().expect("a directory");
    let (passwords, _fake) = passwords(directory.path()).await;

    let connected = passwords
        .connect(&credentials(CLIENT_SECRET, MASTER_PASSWORD))
        .await
        .expect("signs in and unlocks");
    assert_eq!(connected.account_email, "elysia@example.com");
    assert_eq!(connected.collections.len(), 2);
    assert!(passwords.is_ready());

    let found = passwords
        .find(ELYSIA_COLLECTION, Some("pizza"), None)
        .await
        .expect("a search");
    let names: Vec<&str> = found.iter().map(|summary| summary.name.as_str()).collect();
    assert_eq!(names, ["Pizza"], "the bank login is in another collection");

    let login = passwords
        .login(ELYSIA_COLLECTION, PIZZA)
        .await
        .expect("a login in the collection");
    assert_eq!(login.password.as_deref(), Some(PIZZA_PASSWORD));
    let refused = passwords
        .login(ELYSIA_COLLECTION, BANK)
        .await
        .expect_err("another collection's item");
    assert!(matches!(refused, VaultError::NotInCollection(id) if id == BANK));
    let totp = passwords
        .totp(ELYSIA_COLLECTION, BANK)
        .await
        .expect_err("another collection's code");
    assert!(matches!(totp, VaultError::NotInCollection(_)));

    let code = passwords
        .totp(ELYSIA_COLLECTION, PIZZA)
        .await
        .expect("a code");
    assert_eq!(code.expose_secret(), TOTP_CODE);

    let mut revealed = Vec::new();
    passwords
        .revealed()
        .each(|secret| revealed.push(secret.to_owned()));
    assert_eq!(revealed, [PIZZA_PASSWORD, TOTP_CODE]);

    passwords.disconnect().await;
    assert_eq!(passwords.status().state, VaultState::Unconfigured);
    let after = passwords
        .find(ELYSIA_COLLECTION, None, None)
        .await
        .expect_err("nothing is readable once removed");
    assert!(matches!(after, VaultError::NotConfigured));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn refused_credentials_say_why_and_leave_nothing_running() {
    let directory = tempfile::tempdir().expect("a directory");
    let (passwords, _fake) = passwords(directory.path()).await;

    let secret = passwords
        .connect(&credentials("wrong-secret", MASTER_PASSWORD))
        .await
        .expect_err("a wrong client secret");
    assert!(
        secret.to_string().contains("client_secret is incorrect"),
        "{secret}"
    );
    assert_eq!(
        passwords.status().state,
        VaultState::Unconfigured,
        "nothing is stored"
    );

    let master = passwords
        .connect(&credentials(CLIENT_SECRET, "wrong master password"))
        .await
        .expect_err("a wrong master password");
    assert!(
        master.to_string().contains("Invalid master password"),
        "{master}"
    );
    assert!(
        passwords.inner.process.lock().await.is_none(),
        "bw serve was stopped"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_stored_vault_is_brought_back_without_signing_in_again() {
    let directory = tempfile::tempdir().expect("a directory");
    let (passwords, _fake) = passwords(directory.path()).await;
    let connected = passwords
        .connect(&credentials(CLIENT_SECRET, MASTER_PASSWORD))
        .await
        .expect("signs in");

    let mut connection = passwords.inner.database.get().await.expect("a connection");
    let (server, server_url) = credentials(CLIENT_SECRET, MASTER_PASSWORD).server.stored();
    password_vault::save(
        &mut connection,
        &passwords.inner.cipher,
        &password_vault::VerifiedVault {
            server,
            server_url,
            client_id: "user.6c2b0d8e-2f9e-4c52-9a8e-0e3c1f6e8d11".to_owned(),
            // Wrong on purpose: signing in again would be refused, so a vault that comes
            // back proves the CLI was only unlocked.
            client_secret: SecretString::from("wrong-secret-on-purpose"),
            master_password: SecretString::from(MASTER_PASSWORD),
            account_email: connected.account_email,
        },
        None,
        TEST_PERSON_ID,
    )
    .await
    .expect("saved");
    drop(connection);

    // The CLI is still signed in as the stored account, so restoring only unlocks.
    std::fs::write(directory.path().join("state/fake-state"), "locked").expect("written");
    passwords.restore().await;
    assert!(passwords.is_ready(), "{:?}", passwords.status());
}

#[test]
fn revealed_secrets_are_remembered_once_and_bounded() {
    let revealed = Revealed::default();
    revealed.add(&SecretString::from("first-secret"));
    revealed.add(&SecretString::from("first-secret"));
    let mut seen = Vec::new();
    revealed.each(|secret| seen.push(secret.to_owned()));
    assert_eq!(seen, ["first-secret"]);

    for index in 0..=MAX_REVEALED {
        revealed.add(&SecretString::from(format!("secret-{index}")));
    }
    let mut count = 0;
    let mut first = None;
    revealed.each(|secret| {
        count += 1;
        first.get_or_insert_with(|| secret.to_owned());
    });
    assert_eq!(count, MAX_REVEALED);
    assert_eq!(first.as_deref(), Some("secret-1"), "the oldest go first");
}

#[test]
fn servers_round_trip_through_storage() {
    for server in [
        Server::BitwardenUs,
        Server::BitwardenEu,
        Server::SelfHosted(Url::parse("https://vault.example.com").expect("a URL")),
    ] {
        let (kind, url) = server.stored();
        assert_eq!(
            Server::from_stored(kind, url.as_deref()).expect("parses"),
            server
        );
    }
    assert_eq!(
        Server::SelfHosted(Url::parse("https://vault.example.com/").expect("a URL"))
            .stored()
            .1
            .as_deref(),
        Some("https://vault.example.com"),
        "stored as the origin, without a trailing slash"
    );
    Server::from_stored(PasswordServer::SelfHosted, None).expect_err("refused");
}
