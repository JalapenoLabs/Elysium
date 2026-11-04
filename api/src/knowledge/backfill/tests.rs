// Copyright © 2026 Jalapeno Labs

//! The backfill against Postgres (#93): records stored before they were indexed on write are
//! indexed across batches, a walk cut short resumes after its last batch, and a finished walk
//! is not walked again until the version it walked to changes.
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
use crate::models::knowledge_backfill::{self, BackfillSource, Walk};
use crate::test_support::migrated_database;

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
        .walk(&CancellationToken::new())
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
        .walk(&CancellationToken::new())
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
