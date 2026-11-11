// Copyright © 2026 Jalapeno Labs

//! One connector read by two loops, as a Teams connector is by its chat reader and the meetings
//! listing: each records its own keys of the cursor, and neither drops nor rolls back the
//! other's, even when it read the cursor before the other wrote.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs it.

use chrono::Utc;
use secrecy::SecretString;
use serde_json::{Value, json};

use crate::connectors::ConnectorKind;
use crate::models::connector::{self, AccountRef, NewConnector, PollOutcome};
use crate::models::token_account::{self, NewTokenAccount};
use crate::test_support::{TEST_PERSON_ID, cipher, migrated_database};

fn outcome(cursor: Value) -> PollOutcome {
    PollOutcome {
        sign_in_state: "signed_in".to_owned(),
        cursor,
        last_polled_at: Some(Utc::now()),
        last_error: None,
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn two_loops_keep_their_own_keys_of_one_cursor() {
    let (_url, mut connection) = migrated_database().await;
    let cipher = cipher();
    let account = token_account::create(
        &mut connection,
        NewTokenAccount::new(
            &cipher,
            ConnectorKind::Fireflies.as_str(),
            "Fireflies",
            &SecretString::from("key"),
            "tester@example.com",
            TEST_PERSON_ID,
        ),
    )
    .await
    .expect("the account is created");
    let created = connector::create(
        &mut connection,
        NewConnector::new(
            ConnectorKind::Fireflies.as_str(),
            AccountRef::Token(account.id),
            TEST_PERSON_ID,
        ),
    )
    .await
    .expect("the connector is created");

    // The chat reader records its keys, then the listing its own, then the chat reader again.
    let chats = json!({ "baselined": true, "reader": { "since": 1 } });
    connector::record_poll(&mut connection, created.id, outcome(chats))
        .await
        .expect("the chat read is recorded");
    let listing = json!({ "since": "2026-11-10T17:30:15+00:00" });
    connector::record_poll(&mut connection, created.id, outcome(listing))
        .await
        .expect("the listing is recorded");
    let next_chats = json!({ "baselined": true, "reader": { "since": 2 } });
    let stored = connector::record_poll(&mut connection, created.id, outcome(next_chats))
        .await
        .expect("the next chat read is recorded");
    assert_eq!(
        stored.cursor,
        json!({
            "baselined": true,
            "reader": { "since": 2 },
            "since": "2026-11-10T17:30:15+00:00",
        })
    );

    // A read that changed nothing of the cursor leaves all of it.
    let failed = connector::record_poll(&mut connection, created.id, outcome(json!({})))
        .await
        .expect("a failed read is recorded");
    assert_eq!(failed.cursor, stored.cursor);
}
