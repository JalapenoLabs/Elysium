// Copyright © 2026 Jalapeno Labs

//! The migration that gives every Fireflies key a connector (#151), against data laid out the
//! way production can hold it before: keys whose connectors were removed, with the meetings
//! those connectors read kept without them. Up must give each key a paused connector and hand a
//! person's meetings back when only one key can have read them; down must leave the keys alone
//! again without touching a connector its person made.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs it.

use diesel::QueryableByName;
use diesel::sql_types::{Bool, Nullable, Uuid as SqlUuid};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl, SimpleAsyncConnection};
use secrecy::{ExposeSecret, SecretString};
use uuid::{Uuid, uuid};

use super::migrations::{self, MIGRATIONS, MigrationCommand};
use crate::test_support::empty_database;

/// The migration under test.
const ADOPT: &str = "2026-11-04-010000-0000_give_every_fireflies_key_a_connector";

/// Ada's one key left without a connector.
const ADA_KEY: Uuid = uuid!("00000000-0000-7000-8000-0000000f0001");
/// Ada's key that still has its connector, [`ADA_CONNECTOR`].
const ADA_LIVE_KEY: Uuid = uuid!("00000000-0000-7000-8000-0000000f0002");
const ADA_CONNECTOR: Uuid = uuid!("00000000-0000-7000-8000-0000000fc001");
/// Grace's two keys left without a connector.
const GRACE_KEYS: [Uuid; 2] = [
    uuid!("00000000-0000-7000-8000-0000000f0003"),
    uuid!("00000000-0000-7000-8000-0000000f0004"),
];
/// A YouTrack key left without a connector, which this migration is not about.
const YOUTRACK_KEY: Uuid = uuid!("00000000-0000-7000-8000-0000000f0005");

/// Ada's meetings kept without a connector: one read twice (the older and the newer copy), and
/// one read once.
const ADA_OLDER_COPY: Uuid = uuid!("00000000-0000-7000-8000-0000000e0001");
const ADA_NEWER_COPY: Uuid = uuid!("00000000-0000-7000-8000-0000000e0002");
const ADA_OTHER_MEETING: Uuid = uuid!("00000000-0000-7000-8000-0000000e0003");
/// Grace's meeting kept without a connector.
const GRACE_MEETING: Uuid = uuid!("00000000-0000-7000-8000-0000000e0004");

const ORPHANED_LAYOUT: &str = "
INSERT INTO users (id, kratos_identity_id, email, name, role, status, approved_at) VALUES
 ('0199a3c4-0000-7000-8000-00000000ada0', '0199a3c4-0000-7000-8000-00000000ada1',
  'ada@example.com', 'Ada', 'admin', 'active', now()),
 ('0199a3c4-0000-7000-8000-00000000c0a0', '0199a3c4-0000-7000-8000-00000000c0a1',
  'grace@example.com', 'Grace', 'member', 'active', now());
INSERT INTO storage_locations (id, name, kind, bunny_zone, bunny_region, storage_limit_bytes,
                               access_key_encrypted, created_by)
VALUES ('00000000-0000-7000-8000-00000000500a', 'Recordings', 'bunny', 'recordings', 'london',
        NULL, decode(repeat('cd', 60), 'hex'), '0199a3c4-0000-7000-8000-00000000ada0');
INSERT INTO token_accounts (id, kind, name, credential_encrypted, site_url, created_by) VALUES
 ('00000000-0000-7000-8000-0000000f0001', 'fireflies', 'Fireflies',
  decode(repeat('ab', 60), 'hex'), NULL, '0199a3c4-0000-7000-8000-00000000ada0'),
 ('00000000-0000-7000-8000-0000000f0002', 'fireflies', 'Fireflies',
  decode(repeat('ab', 60), 'hex'), NULL, '0199a3c4-0000-7000-8000-00000000ada0'),
 ('00000000-0000-7000-8000-0000000f0003', 'fireflies', 'Fireflies',
  decode(repeat('ab', 60), 'hex'), NULL, '0199a3c4-0000-7000-8000-00000000c0a0'),
 ('00000000-0000-7000-8000-0000000f0004', 'fireflies', 'Fireflies',
  decode(repeat('ab', 60), 'hex'), NULL, '0199a3c4-0000-7000-8000-00000000c0a0'),
 ('00000000-0000-7000-8000-0000000f0005', 'youtrack', 'YouTrack',
  decode(repeat('ab', 60), 'hex'), 'https://acme.youtrack.cloud',
  '0199a3c4-0000-7000-8000-00000000ada0');
INSERT INTO connectors (id, kind, token_account_id, storage_location_id, created_by) VALUES
 ('00000000-0000-7000-8000-0000000fc001', 'fireflies', '00000000-0000-7000-8000-0000000f0002',
  '00000000-0000-7000-8000-00000000500a', '0199a3c4-0000-7000-8000-00000000ada0');
INSERT INTO meetings (id, connector_id, source_kind, external_id, started_at, created_by,
                      created_at) VALUES
 ('00000000-0000-7000-8000-0000000e0001', NULL, 'fireflies', 'shared', now(),
  '0199a3c4-0000-7000-8000-00000000ada0', now() - interval '3 days'),
 ('00000000-0000-7000-8000-0000000e0002', NULL, 'fireflies', 'shared', now(),
  '0199a3c4-0000-7000-8000-00000000ada0', now() - interval '1 day'),
 ('00000000-0000-7000-8000-0000000e0003', NULL, 'fireflies', 'standup', now(),
  '0199a3c4-0000-7000-8000-00000000ada0', now()),
 ('00000000-0000-7000-8000-0000000e0004', NULL, 'fireflies', 'review', now(),
  '0199a3c4-0000-7000-8000-00000000c0a0', now());
";

/// A connector as the test reads it back.
#[derive(Debug, PartialEq, Eq, QueryableByName)]
struct ConnectorRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = Bool)]
    is_active: bool,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    storage_location_id: Option<Uuid>,
}

#[derive(QueryableByName)]
struct MeetingRow {
    #[diesel(sql_type = Nullable<SqlUuid>)]
    connector_id: Option<Uuid>,
}

/// The connectors that own `token_account_id`.
async fn connectors_of(
    connection: &mut AsyncPgConnection,
    token_account_id: Uuid,
) -> Vec<ConnectorRow> {
    diesel::sql_query(
        "SELECT id, is_active, storage_location_id FROM connectors WHERE token_account_id = $1",
    )
    .bind::<SqlUuid, _>(token_account_id)
    .load(connection)
    .await
    .expect("the connectors load")
}

async fn connector_of_meeting(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> Option<Uuid> {
    diesel::sql_query("SELECT connector_id FROM meetings WHERE id = $1")
        .bind::<SqlUuid, _>(meeting_id)
        .get_result::<MeetingRow>(connection)
        .await
        .expect("the meeting is still there")
        .connector_id
}

/// How many migrations come after [`ADOPT`], plus [`ADOPT`] itself, so reverting that many
/// lands just before it however many land after it.
fn from_adopt() -> u32 {
    let names: Vec<String> =
        diesel::migration::MigrationSource::<diesel::pg::Pg>::migrations(&MIGRATIONS)
            .expect("embedded migrations load")
            .iter()
            .map(|migration| migration.name().to_string())
            .collect();
    let position = names
        .iter()
        .position(|name| name == ADOPT)
        .expect("the adopting migration is embedded");
    u32::try_from(names.len() - position).expect("a handful of migrations")
}

/// A database just past [`ADOPT`], applied to [`ORPHANED_LAYOUT`].
async fn adopted_layout() -> (SecretString, AsyncPgConnection) {
    let url = empty_database().await;
    migrations::execute(url.clone(), MigrationCommand::Run)
        .await
        .expect("up");
    migrations::execute(
        url.clone(),
        MigrationCommand::Revert {
            count: from_adopt(),
        },
    )
    .await
    .expect("back to just before the migration");
    let mut connection = AsyncPgConnection::establish(url.expose_secret())
        .await
        .expect("connects");
    connection
        .batch_execute(ORPHANED_LAYOUT)
        .await
        .expect("the orphaned layout is stored");

    migrations::execute(url.clone(), MigrationCommand::Run)
        .await
        .expect("the migration applies to orphaned keys");
    (url, connection)
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn every_fireflies_key_gets_a_paused_connector_and_its_meetings_back() {
    let (_url, mut connection) = adopted_layout().await;

    // Ada's lone key has a paused connector that keeps nothing until she picks a location.
    let adopted = connectors_of(&mut connection, ADA_KEY).await;
    let [ada] = adopted.as_slice() else {
        panic!("one connector owns Ada's key, found {adopted:?}");
    };
    assert!(!ada.is_active);
    assert_eq!(ada.storage_location_id, None);

    // Her meetings come back to it, the newer copy of the one read twice among them.
    assert_eq!(
        connector_of_meeting(&mut connection, ADA_NEWER_COPY).await,
        Some(ada.id)
    );
    assert_eq!(
        connector_of_meeting(&mut connection, ADA_OTHER_MEETING).await,
        Some(ada.id)
    );
    assert_eq!(
        connector_of_meeting(&mut connection, ADA_OLDER_COPY).await,
        None
    );

    // Grace's two keys each get one, and her meeting stays unowned, since either could have
    // read it.
    for key in GRACE_KEYS {
        assert_eq!(connectors_of(&mut connection, key).await.len(), 1);
    }
    assert_eq!(
        connector_of_meeting(&mut connection, GRACE_MEETING).await,
        None
    );

    // The connector Ada already had is untouched, and a YouTrack key is not this migration's.
    assert_eq!(
        connectors_of(&mut connection, ADA_LIVE_KEY).await,
        [ConnectorRow {
            id: ADA_CONNECTOR,
            is_active: true,
            storage_location_id: Some(uuid!("00000000-0000-7000-8000-00000000500a")),
        }]
    );
    assert!(
        connectors_of(&mut connection, YOUTRACK_KEY)
            .await
            .is_empty()
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn reverting_leaves_the_keys_without_connectors_and_keeps_the_rest() {
    let (url, mut connection) = adopted_layout().await;

    migrations::execute(
        url.clone(),
        MigrationCommand::Revert {
            count: from_adopt(),
        },
    )
    .await
    .expect("the migration reverts");

    assert!(connectors_of(&mut connection, ADA_KEY).await.is_empty());
    for key in GRACE_KEYS {
        assert!(connectors_of(&mut connection, key).await.is_empty());
    }
    assert_eq!(
        connector_of_meeting(&mut connection, ADA_NEWER_COPY).await,
        None
    );
    assert_eq!(connectors_of(&mut connection, ADA_LIVE_KEY).await.len(), 1);
}
