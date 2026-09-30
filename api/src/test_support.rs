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
    .bind::<diesel::sql_types::Uuid, _>(Uuid::now_v7())
    .execute(&mut connection)
    .await
    .expect("the test person is created");
    (url, connection)
}

/// A cipher under a fresh random key.
pub fn cipher() -> Cipher {
    Cipher::from_base64_key(&SecretString::from(generate_key())).expect("generated keys are valid")
}
