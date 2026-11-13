// Copyright © 2026 Jalapeno Labs

//! Who reads what agents wrote from meetings (#278), against Postgres: a record naming meetings is
//! read only by whoever may read every one of them, by the Diesel rule and its SQL twin alike, for
//! every reach; a session's turn carries what its earlier turns read; and a record an agent is
//! handed names what it says, an excerpt naming no meeting being handed to none.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use chrono::Utc;
use diesel::QueryableByName;
use diesel::sql_types::{Array, Text, Uuid as SqlUuid};
use serde_json::{Value, json};

use super::*;
use crate::models::coding_session::{self, NewCodingSession};
use crate::models::meeting::{Reach, Visibility};
use crate::models::meeting_share;
use crate::models::project::{self, NewProject};
use crate::models::session_turn::{self, NewSessionTurn};
use crate::models::timeline_record::{self, NewTimelineRecord};
use crate::models::user::SYSTEM_USER_ID;
use crate::test_support::{TEST_PERSON_ID, migrated_database};

/// A meeting of `owner`'s, read as `visibility` says.
async fn meeting_of(
    connection: &mut AsyncPgConnection,
    owner: Uuid,
    visibility: Visibility,
) -> Uuid {
    let (found, _capture) = crate::models::meeting::tests::stored(connection).await;
    diesel::sql_query("UPDATE meetings SET created_by = $1, visibility = $2 WHERE id = $3")
        .bind::<SqlUuid, _>(owner)
        .bind::<Text, _>(visibility.as_str())
        .bind::<SqlUuid, _>(found.id)
        .execute(connection)
        .await
        .expect("the meeting is set up");
    found.id
}

/// A record of `kind` an agent might write, naming `meetings`; `metadata` as the record holds it.
async fn record_naming(
    connection: &mut AsyncPgConnection,
    kind: &str,
    metadata: Value,
    meetings: &[Uuid],
) -> Uuid {
    let stored = timeline_record::insert_new(
        connection,
        NewTimelineRecord {
            id: Uuid::now_v7(),
            source_kind: kind.to_owned(),
            connector_id: None,
            external_id: None,
            occurred_at: Utc::now(),
            actor: "Project Manager".to_owned(),
            title: "Planned the work".to_owned(),
            body: String::new(),
            url: None,
            media: None,
            metadata,
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("stored")
    .expect("new");
    name_on_record(connection, stored.id, meetings)
        .await
        .expect("named");
    stored.id
}

/// A record's id, as the SQL twin of the rule finds it.
#[derive(QueryableByName)]
struct Found {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn its_sql_reads_records_as_its_query_does() {
    let (_url, mut connection) = migrated_database().await;
    let someone = SYSTEM_USER_ID;
    let own = meeting_of(&mut connection, TEST_PERSON_ID, Visibility::Private).await;
    let shared = meeting_of(&mut connection, someone, Visibility::Private).await;
    meeting_share::add(&mut connection, shared, TEST_PERSON_ID, someone)
        .await
        .expect("shared");
    let private = meeting_of(&mut connection, someone, Visibility::Private).await;
    let unchecked = meeting_of(&mut connection, someone, Visibility::Unchecked).await;
    let public = meeting_of(&mut connection, someone, Visibility::Public).await;

    let named_nothing = record_naming(&mut connection, "project_manager", json!({}), &[]).await;
    let from_own = record_naming(&mut connection, "project_manager", json!({}), &[own]).await;
    let from_shared = record_naming(&mut connection, "project_manager", json!({}), &[shared]).await;
    let from_private =
        record_naming(&mut connection, "project_manager", json!({}), &[private]).await;
    let from_unchecked =
        record_naming(&mut connection, "project_manager", json!({}), &[unchecked]).await;
    let from_public = record_naming(&mut connection, "project_manager", json!({}), &[public]).await;
    // Written from two meetings, the reader may read one of: it needs both.
    let from_public_and_private = record_naming(
        &mut connection,
        "project_manager",
        json!({}),
        &[public, private],
    )
    .await;
    let from_own_and_shared = record_naming(
        &mut connection,
        "project_manager",
        json!({}),
        &[own, shared],
    )
    .await;
    let every = [
        named_nothing,
        from_own,
        from_shared,
        from_private,
        from_unchecked,
        from_public,
        from_public_and_private,
        from_own_and_shared,
    ];

    let expected = [
        (Reach::Every, every.to_vec()),
        (
            Reach::Public,
            vec![
                named_nothing,
                from_own,
                from_shared,
                from_public,
                from_own_and_shared,
            ],
        ),
        (
            Reach::Shared,
            vec![named_nothing, from_own, from_shared, from_own_and_shared],
        ),
    ];
    for (reach, reads) in expected {
        let reader = Reader {
            id: TEST_PERSON_ID,
            reach,
        };
        let by_query: HashSet<Uuid> = timeline_records::table
            .filter(timeline_records::id.eq_any(every))
            .filter(readable_records(reader))
            .select(timeline_records::id)
            .load(&mut connection)
            .await
            .expect("reads")
            .into_iter()
            .collect();
        let by_sql: HashSet<Uuid> = diesel::sql_query(concat!(
            "SELECT candidates.id FROM timeline_records candidates
             WHERE candidates.id = ANY($1::uuid[]) AND ",
            readable_record_sql!("candidates.id", "$2", "$3"),
        ))
        .bind::<Array<SqlUuid>, _>(&every[..])
        .bind::<SqlUuid, _>(reader.id)
        .bind::<Text, _>(reach.as_str())
        .load::<Found>(&mut connection)
        .await
        .expect("reads")
        .into_iter()
        .map(|found| found.id)
        .collect();
        let among = readable_among(&mut connection, &every, reader)
            .await
            .expect("reads");
        assert_eq!(by_query, reads.into_iter().collect(), "{reach:?}");
        assert_eq!(by_sql, by_query, "{reach:?}");
        assert_eq!(among, by_query, "{reach:?}");
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn opening_a_meeting_opens_what_was_written_from_it_and_closing_it_closes_it() {
    let (_url, mut connection) = migrated_database().await;
    let private = meeting_of(&mut connection, SYSTEM_USER_ID, Visibility::Private).await;
    let from_private =
        record_naming(&mut connection, "project_manager", json!({}), &[private]).await;

    // Who reads it is asked as each request runs, with nothing written again.
    let member = Reader {
        id: TEST_PERSON_ID,
        reach: Reach::Public,
    };
    let reads = async |connection: &mut AsyncPgConnection| {
        readable_among(connection, &[from_private], member)
            .await
            .expect("reads")
            .contains(&from_private)
    };
    assert!(!reads(&mut connection).await);
    crate::models::meeting::set_visibility(&mut connection, private, Visibility::Public)
        .await
        .expect("opened");
    assert!(reads(&mut connection).await, "an open meeting opens it");
    crate::models::meeting::set_visibility(&mut connection, private, Visibility::Private)
        .await
        .expect("closed");
    assert!(!reads(&mut connection).await, "a closed meeting closes it");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_sessions_turn_carries_what_its_earlier_turns_read() {
    let (_url, mut connection) = migrated_database().await;
    let private = meeting_of(&mut connection, SYSTEM_USER_ID, Visibility::Private).await;
    let project_id = project::create(
        &mut connection,
        &NewProject {
            created_by: TEST_PERSON_ID,
            name: "Reorg".to_owned(),
            description: String::new(),
        },
    )
    .await
    .expect("a project")
    .id;
    let id = coding_session::reserve_id(&mut connection)
        .await
        .expect("a number");
    let session = coding_session::create(
        &mut connection,
        &NewCodingSession {
            id,
            project_id: Some(project_id),
            title: "Look into the reorg".to_owned(),
            github_credential_id: None,
            action_item_id: None,
            studio_item_id: None,
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("a session");
    let mut turns = Vec::new();
    for prompt in ["Read the one-to-one.", "Now write the plan."] {
        let turn = session_turn::queue(
            &mut connection,
            &NewSessionTurn {
                id: Uuid::now_v7(),
                session_id: session.id,
                prompt,
                system_prompt_append: None,
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("queued");
        turns.push(turn.id);
    }

    // The first turn read the meeting through a tool; the second starts afterwards.
    keep(
        &mut connection,
        Turn::Session {
            session_id: session.id,
            turn_id: turns[0],
        },
        &[private],
    )
    .await
    .expect("kept");
    carry_session(&mut connection, turns[1])
        .await
        .expect("carried");
    assert_eq!(
        of_turn(
            &mut connection,
            Turn::Session {
                session_id: session.id,
                turn_id: turns[1],
            }
        )
        .await
        .expect("read"),
        [private],
        "it resumes the turn that read the meeting, so it carries it"
    );
    // Carrying again changes nothing, and a turn carries nothing of its own later turns.
    carry_session(&mut connection, turns[1])
        .await
        .expect("carried");
    carry_session(&mut connection, turns[0])
        .await
        .expect("carried");
    assert_eq!(
        of_turn(
            &mut connection,
            Turn::Session {
                session_id: session.id,
                turn_id: turns[0],
            }
        )
        .await
        .expect("read"),
        [private]
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn an_agent_is_handed_what_a_record_names_and_never_an_excerpt_naming_no_meeting() {
    let (_url, mut connection) = migrated_database().await;
    let private = meeting_of(&mut connection, SYSTEM_USER_ID, Visibility::Private).await;
    // An excerpt names its own meeting as it is stored, from its metadata.
    let excerpt = record_naming(
        &mut connection,
        "meeting",
        json!({ "meetingId": private }),
        &[],
    )
    .await;
    let unplaced = record_naming(&mut connection, "meeting", json!({}), &[]).await;
    let written = record_naming(&mut connection, "project_manager", json!({}), &[private]).await;
    let mail = record_naming(&mut connection, "outlook", json!({}), &[]).await;

    let records = timeline_record::find_many(&mut connection, &[excerpt, unplaced, written, mail])
        .await
        .expect("read");
    let handed = for_agent(&mut connection, records, |record| record)
        .await
        .expect("handed");
    let mut handed: Vec<(Uuid, Vec<Uuid>)> = handed
        .into_iter()
        .map(|(record, meetings)| (record.id, meetings))
        .collect();
    handed.sort();
    let mut expected = vec![
        (excerpt, vec![private]),
        (written, vec![private]),
        (mail, Vec::new()),
    ];
    expected.sort();
    assert_eq!(handed, expected);
}
