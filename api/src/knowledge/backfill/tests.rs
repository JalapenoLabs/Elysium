// Copyright © 2026 Jalapeno Labs

//! The backfill against Postgres (#93, #193): records stored before they were indexed on write
//! are indexed across batches, turn summaries written before they went on the timeline are put
//! there once, a walk cut short resumes after its last batch, and a finished walk is not walked
//! again until the version it walked to changes.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl, SimpleAsyncConnection};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{BATCH_SIZE, Backfill};
use crate::connections::connect_postgres;
use crate::database::schema::{knowledge_backfills, knowledge_documents, timeline_records};
use crate::knowledge::records::INDEXED_AS;
use crate::models::coding_session::{self, NewCodingSession};
use crate::models::knowledge_backfill::{self, BackfillSource, Walk};
use crate::models::project::{self, NewProject};
use crate::models::turn_summary;
use crate::sessions::timeline;
use crate::test_support::{TEST_PERSON_ID, cipher, migrated_database, studio_item};

/// Stores `count` Slack messages straight into the table, as records were stored before they
/// were indexed on write, so none of them has a document.
async fn store_unindexed(connection: &mut AsyncPgConnection, count: i64) {
    connection
        .batch_execute(&format!(
            "INSERT INTO timeline_records (id, source_kind, external_id, occurred_at, title, body, \
                                           created_by) \
             SELECT uuidv7(), 'slack', 'C1/' || n, now(), '#launch', 'Message ' || n, \
                    '0199a3c4-0000-7000-8000-00000000beef' \
             FROM generate_series(1, {count}) AS n"
        ))
        .await
        .expect("records are stored");
}

/// The ids of the records the index holds a document for, in id order.
async fn indexed_ids(connection: &mut AsyncPgConnection) -> Vec<Uuid> {
    let ids: Vec<Option<Uuid>> = knowledge_documents::table
        .order(knowledge_documents::timeline_record_id.asc())
        .select(knowledge_documents::timeline_record_id)
        .load(connection)
        .await
        .expect("the index is read");
    ids.into_iter().flatten().collect()
}

async fn record_ids(connection: &mut AsyncPgConnection) -> Vec<Uuid> {
    timeline_records::table
        .order(timeline_records::id.asc())
        .select(timeline_records::id)
        .load(connection)
        .await
        .expect("the records are read")
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_walk_indexes_every_record_across_batches_and_then_never_walks_again() {
    let (url, mut connection) = migrated_database().await;
    store_unindexed(&mut connection, BATCH_SIZE + 20).await;
    assert!(indexed_ids(&mut connection).await.is_empty());

    let backfill = Backfill::new(connect_postgres(&url, 2).await.expect("a pool"));
    backfill
        .walk(BackfillSource::TimelineRecord, &CancellationToken::new())
        .await
        .expect("the walk succeeds");

    assert_eq!(
        indexed_ids(&mut connection).await,
        record_ids(&mut connection).await
    );
    assert_eq!(
        knowledge_backfill::begin(&mut connection, BackfillSource::TimelineRecord, INDEXED_AS)
            .await
            .expect("read"),
        Walk::Finished
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_walk_cut_short_resumes_after_its_last_batch() {
    let (url, mut connection) = migrated_database().await;
    store_unindexed(&mut connection, 30).await;
    let ids = record_ids(&mut connection).await;

    // A walk that indexed the first ten and stopped.
    knowledge_backfill::begin(&mut connection, BackfillSource::TimelineRecord, INDEXED_AS)
        .await
        .expect("begun");
    knowledge_backfill::advance(&mut connection, BackfillSource::TimelineRecord, ids[9])
        .await
        .expect("advanced");

    let backfill = Backfill::new(connect_postgres(&url, 2).await.expect("a pool"));
    backfill
        .walk(BackfillSource::TimelineRecord, &CancellationToken::new())
        .await
        .expect("the walk succeeds");

    assert_eq!(indexed_ids(&mut connection).await, ids[10..]);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_new_version_walks_from_the_first_record_and_an_older_one_leaves_it_be() {
    let (_url, mut connection) = migrated_database().await;
    let source = BackfillSource::TimelineRecord;
    let through = Uuid::now_v7();

    assert_eq!(
        knowledge_backfill::begin(&mut connection, source, 1)
            .await
            .expect("begun"),
        Walk::After(None)
    );
    knowledge_backfill::advance(&mut connection, source, through)
        .await
        .expect("advanced");
    assert_eq!(
        knowledge_backfill::begin(&mut connection, source, 1)
            .await
            .expect("resumed"),
        Walk::After(Some(through)),
        "the same version resumes"
    );
    knowledge_backfill::finish(&mut connection, source)
        .await
        .expect("finished");

    assert_eq!(
        knowledge_backfill::begin(&mut connection, source, 2)
            .await
            .expect("begun"),
        Walk::After(None),
        "a new version walks everything again"
    );
    knowledge_backfill::finish(&mut connection, source)
        .await
        .expect("finished");
    assert_eq!(
        knowledge_backfill::begin(&mut connection, source, 1)
            .await
            .expect("read"),
        Walk::Finished,
        "a release rolled back leaves the newer walk's text in place"
    );
    let version: Option<i32> = knowledge_backfills::table
        .find(source.as_str())
        .select(knowledge_backfills::version)
        .first(&mut connection)
        .await
        .expect("read");
    assert_eq!(version, Some(2));
}

/// Stores `count` summaries of a new session on a new project straight into the table, as
/// summaries were stored before they were put on the timeline, and answers the session's number.
async fn store_summaries_off_the_timeline(connection: &mut AsyncPgConnection, count: i64) -> i64 {
    let project = project::create(
        connection,
        &NewProject {
            created_by: TEST_PERSON_ID,
            name: format!("Project {}", Uuid::now_v7()),
            description: String::new(),
        },
    )
    .await
    .expect("a project");
    let id = coding_session::reserve_id(connection).await.expect("an id");
    coding_session::create(
        connection,
        &NewCodingSession {
            created_by: TEST_PERSON_ID,
            id,
            project_id: Some(project.id),
            title: "Login form".to_owned(),
            github_credential_id: None,
            action_item_id: None,
            studio_item_id: None,
        },
    )
    .await
    .expect("a session");
    connection
        .batch_execute(&format!(
            "INSERT INTO turn_summaries (session_id, turn_id, summary) \
             SELECT {id}, uuidv7(), 'Fixed part ' || n || ' of the form.' \
             FROM generate_series(1, {count}) AS n"
        ))
        .await
        .expect("summaries are stored");
    id
}

/// The turns whose summaries have a record, in turn order.
async fn recorded_turns(connection: &mut AsyncPgConnection) -> Vec<Uuid> {
    let turns: Vec<Option<Uuid>> = timeline_records::table
        .filter(timeline_records::turn_summary_turn_id.is_not_null())
        .order(timeline_records::turn_summary_turn_id.asc())
        .select(timeline_records::turn_summary_turn_id)
        .load(connection)
        .await
        .expect("the records are read");
    turns.into_iter().flatten().collect()
}

// #193: summaries written before they went on the timeline are put there once, across batches.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_walk_puts_every_earlier_summary_on_its_project_once_across_batches() {
    let (url, mut connection) = migrated_database().await;
    let session_id = store_summaries_off_the_timeline(&mut connection, BATCH_SIZE + 5).await;
    // A Studio session on an item with no project: walked past, and kept off every timeline.
    let item = studio_item(&mut connection, &cipher()).await;
    let studio_session = coding_session::reserve_id(&mut connection)
        .await
        .expect("an id");
    coding_session::create(
        &mut connection,
        &NewCodingSession {
            created_by: TEST_PERSON_ID,
            id: studio_session,
            project_id: None,
            title: "Banana".to_owned(),
            github_credential_id: None,
            action_item_id: None,
            studio_item_id: Some(item.id),
        },
    )
    .await
    .expect("a Studio session");
    turn_summary::record(&mut connection, studio_session, Uuid::now_v7(), "Drew it.")
        .await
        .expect("recorded");
    let mut expected: Vec<Uuid> = turn_summary::for_session(&mut connection, session_id)
        .await
        .expect("listed")
        .into_iter()
        .map(|summary| summary.turn_id)
        .collect();
    expected.sort();

    let backfill = Backfill::new(connect_postgres(&url, 2).await.expect("a pool"));
    backfill
        .walk(BackfillSource::TurnSummary, &CancellationToken::new())
        .await
        .expect("the walk succeeds");
    assert_eq!(recorded_turns(&mut connection).await, expected);
    // Each record is indexed as it is written.
    let indexed: i64 = knowledge_documents::table
        .filter(knowledge_documents::source.eq(timeline::SOURCE_KIND))
        .count()
        .get_result(&mut connection)
        .await
        .expect("counted");
    assert_eq!(indexed, BATCH_SIZE + 5);
    assert_eq!(
        knowledge_backfill::begin(
            &mut connection,
            BackfillSource::TurnSummary,
            timeline::RECORDED_AS
        )
        .await
        .expect("read"),
        Walk::Finished
    );

    // Walked again from the first, as a release bumping the version would, every summary still
    // has the one record.
    connection
        .batch_execute(
            "UPDATE knowledge_backfills SET through_id = NULL, finished_at = NULL \
             WHERE source = 'turn_summary'",
        )
        .await
        .expect("the walk starts over");
    backfill
        .walk(BackfillSource::TurnSummary, &CancellationToken::new())
        .await
        .expect("the walk succeeds");
    assert_eq!(recorded_turns(&mut connection).await, expected);
}
