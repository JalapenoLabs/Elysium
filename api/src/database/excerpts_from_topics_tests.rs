// Copyright © 2026 Jalapeno Labs

//! The migration that turns the topics meetings were segmented into into excerpts (#241), against
//! topics laid out the way production holds them before: topics cut back to back over a timed
//! meeting, edited since, attributed by Elysia and by a person with their history, and indexed;
//! a meeting in words alone, every topic at `0`; a topic over a silence no word starts in; a topic
//! of a meeting with no final transcript; and records that are no topic at all.
//!
//! Up must make every topic an excerpt over its words, keeping every record, attribution, history
//! entry, and document exactly as it was. Down must hand every row back exactly, and up must apply
//! again.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs it.

use std::collections::HashMap;

use diesel::QueryableByName;
use diesel::sql_types::{Jsonb, Nullable, Text, Uuid as SqlUuid};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl, SimpleAsyncConnection};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use uuid::{Uuid, uuid};

use super::migrations::{self, MIGRATIONS, MigrationCommand};
use crate::models::user::ELYSIA_USER_ID;
use crate::test_support::empty_database;

/// The migration under test.
const EXCERPTS: &str = "2026-11-11-010000-0241_segment_meetings_into_excerpts";

/// A timed meeting cut into two topics back to back, its first word edited since.
const ROADMAP: &str = "00000000-0000-7000-8000-0000000e2001";
/// A meeting in words alone: one topic, at `0`.
const UNTIMED: &str = "00000000-0000-7000-8000-0000000e2002";
/// A meeting with no final transcript.
const WORDLESS: &str = "00000000-0000-7000-8000-0000000e2003";

/// The roadmap's first topic, 0 to 60 s, attributed by Elysia and then by a person.
const INTROS: Uuid = uuid!("00000000-0000-7000-8000-0000000d2001");
/// The roadmap's second topic, 60 s to 120 s.
const LAUNCH: Uuid = uuid!("00000000-0000-7000-8000-0000000d2002");
/// The roadmap's third topic, over a silence from 120 s to 600 s that no word starts in.
const SILENCE: Uuid = uuid!("00000000-0000-7000-8000-0000000d2003");
/// The meeting in words alone's topic.
const SPOKEN: Uuid = uuid!("00000000-0000-7000-8000-0000000d2004");
/// The wordless meeting's topic.
const UNANCHORED: Uuid = uuid!("00000000-0000-7000-8000-0000000d2005");
/// A Slack message, which is no topic.
const MESSAGE: Uuid = uuid!("00000000-0000-7000-8000-0000000d2006");

/// The roadmap's words, by position: the first, `Hi`, replaced by an edit with `Hello`.
const HI: Uuid = uuid!("00000000-0000-7000-8000-0000000a2000");
const HELLO: Uuid = uuid!("00000000-0000-7000-8000-0000000a2001");
const EVERYONE: Uuid = uuid!("00000000-0000-7000-8000-0000000a2002");
const LAUNCH_WORD: Uuid = uuid!("00000000-0000-7000-8000-0000000a2003");
const FRIDAY: Uuid = uuid!("00000000-0000-7000-8000-0000000a2004");
/// The words of the meeting in words alone.
const SPOKEN_FIRST: Uuid = uuid!("00000000-0000-7000-8000-0000000a2101");
const SPOKEN_LAST: Uuid = uuid!("00000000-0000-7000-8000-0000000a2102");

/// What production holds before the excerpts.
const BEFORE_THE_EXCERPTS: &str = r#"
INSERT INTO users (id, kratos_identity_id, email, name, role, status, approved_at) VALUES
 ('0199a3c4-0000-7000-8000-00000000ada0', '0199a3c4-0000-7000-8000-00000000ada1',
  'ada@example.com', 'Ada', 'admin', 'active', now());
INSERT INTO projects (id, name, description, created_by) VALUES
 ('00000000-0000-7000-8000-0000000b2001', 'Launch', '', '0199a3c4-0000-7000-8000-00000000ada0');

INSERT INTO meetings (id, title, started_at, duration_ms, organizer, participants, segments,
                      state, created_by)
VALUES
 ('00000000-0000-7000-8000-0000000e2001', 'Roadmap', '2026-11-01T15:00:00Z', 600000, '', '[]',
  '[{"startMs": 0, "endMs": 60000, "title": "Intros", "summary": "", "projects": []}]', 'done',
  '0199a3c4-0000-7000-8000-00000000ada0'),
 ('00000000-0000-7000-8000-0000000e2002', 'Notes', '2026-11-02T15:00:00Z', NULL, '', '[]',
  '[{"startMs": 0, "endMs": 0, "title": "All", "summary": "", "projects": [],
     "lines": {"first": 0, "last": 1}}]', 'done', '0199a3c4-0000-7000-8000-00000000ada0'),
 ('00000000-0000-7000-8000-0000000e2003', 'Lost', '2026-11-03T15:00:00Z', NULL, '', '[]', '[]',
  'done', '0199a3c4-0000-7000-8000-00000000ada0');

INSERT INTO meeting_captures (id, meeting_id, source_kind, transcript, created_by) VALUES
 ('00000000-0000-7000-8000-0000000c2001', '00000000-0000-7000-8000-0000000e2001', 'upload', '[]',
  '0199a3c4-0000-7000-8000-00000000ada0'),
 ('00000000-0000-7000-8000-0000000c2002', '00000000-0000-7000-8000-0000000e2002', 'upload', '[]',
  '0199a3c4-0000-7000-8000-00000000ada0'),
 ('00000000-0000-7000-8000-0000000c2003', '00000000-0000-7000-8000-0000000e2003', 'upload', '[]',
  '0199a3c4-0000-7000-8000-00000000ada0');

INSERT INTO meeting_events (id, meeting_id, kind, created_by, data) VALUES
 ('00000000-0000-7000-8000-0000000f2001', '00000000-0000-7000-8000-0000000e2001', 'built',
  '00000000-0000-0000-0000-000000000001', '{}'),
 ('00000000-0000-7000-8000-0000000f2002', '00000000-0000-7000-8000-0000000e2001', 'edited',
  '0199a3c4-0000-7000-8000-00000000ada0', '{}'),
 ('00000000-0000-7000-8000-0000000f2003', '00000000-0000-7000-8000-0000000e2002', 'built',
  '00000000-0000-0000-0000-000000000001', '{}');

INSERT INTO meeting_words (id, meeting_id, position, text, speaker, start_ms, end_ms, origin,
                           capture_id, introduced_by, superseded_by)
VALUES
 ('00000000-0000-7000-8000-0000000a2000', '00000000-0000-7000-8000-0000000e2001', 1024, 'Hi',
  'Ada', 1000, 1400, 'source', '00000000-0000-7000-8000-0000000c2001',
  '00000000-0000-7000-8000-0000000f2001', '00000000-0000-7000-8000-0000000f2002'),
 ('00000000-0000-7000-8000-0000000a2001', '00000000-0000-7000-8000-0000000e2001', 1536, 'Hello',
  'Ada', 1000, 1400, 'edit', NULL, '00000000-0000-7000-8000-0000000f2002', NULL),
 ('00000000-0000-7000-8000-0000000a2002', '00000000-0000-7000-8000-0000000e2001', 2048,
  'everyone.', 'Ada', 1500, 2000, 'source', '00000000-0000-7000-8000-0000000c2001',
  '00000000-0000-7000-8000-0000000f2001', NULL),
 ('00000000-0000-7000-8000-0000000a2003', '00000000-0000-7000-8000-0000000e2001', 3072, 'Launch',
  'Grace', 61000, 61400, 'source', '00000000-0000-7000-8000-0000000c2001',
  '00000000-0000-7000-8000-0000000f2001', NULL),
 ('00000000-0000-7000-8000-0000000a2004', '00000000-0000-7000-8000-0000000e2001', 4096,
  'Friday.', 'Grace', 61500, 62000, 'source', '00000000-0000-7000-8000-0000000c2001',
  '00000000-0000-7000-8000-0000000f2001', NULL),
 ('00000000-0000-7000-8000-0000000a2101', '00000000-0000-7000-8000-0000000e2002', 1024, 'Hello.',
  'Ada', 0, 0, 'source', '00000000-0000-7000-8000-0000000c2002',
  '00000000-0000-7000-8000-0000000f2003', NULL),
 ('00000000-0000-7000-8000-0000000a2102', '00000000-0000-7000-8000-0000000e2002', 2048, 'Bye.',
  'Grace', 0, 0, 'source', '00000000-0000-7000-8000-0000000c2002',
  '00000000-0000-7000-8000-0000000f2003', NULL);

INSERT INTO timeline_records (id, source_kind, connector_id, external_id, occurred_at, actor,
                              title, body, media, metadata, created_by, created_at)
VALUES
 ('00000000-0000-7000-8000-0000000d2001', 'meeting', NULL,
  'meeting:00000000-0000-7000-8000-0000000e2001#0', '2026-11-01T15:00:00Z', 'Ada', 'Intros',
  '[0:01] Ada: Hi everyone.', '{"meetingId": "00000000-0000-7000-8000-0000000e2001",
  "startMs": 0, "endMs": 60000, "clip": null}', '{"meetingId":
  "00000000-0000-7000-8000-0000000e2001", "segmentIndex": 0, "summary": "Hellos."}',
  '0199a3c4-0000-7000-8000-00000000ada0', '2026-11-01T16:00:00Z'),
 ('00000000-0000-7000-8000-0000000d2002', 'meeting', NULL,
  'meeting:00000000-0000-7000-8000-0000000e2001#1', '2026-11-01T15:01:00Z', 'Grace', 'Launch',
  '[1:01] Grace: Launch Friday.', '{"meetingId": "00000000-0000-7000-8000-0000000e2001",
  "startMs": 60000, "endMs": 120000, "clip": {"storageLocationId":
  "00000000-0000-7000-8000-00000000510a", "path": "meetings/x/segments/1.mp4", "contentType":
  "video/mp4", "startMs": 60400, "endMs": 119800}}', '{"meetingId":
  "00000000-0000-7000-8000-0000000e2001", "segmentIndex": 1, "summary": "Friday."}',
  '0199a3c4-0000-7000-8000-00000000ada0', '2026-11-01T16:00:00Z'),
 ('00000000-0000-7000-8000-0000000d2003', 'meeting', NULL,
  'meeting:00000000-0000-7000-8000-0000000e2001#2', '2026-11-01T15:02:00Z', '', 'Quiet',
  '', '{"meetingId": "00000000-0000-7000-8000-0000000e2001", "startMs": 120000,
  "endMs": 600000, "clip": null}', '{"meetingId": "00000000-0000-7000-8000-0000000e2001",
  "segmentIndex": 2, "summary": ""}', '0199a3c4-0000-7000-8000-00000000ada0',
  '2026-11-01T16:00:00Z'),
 ('00000000-0000-7000-8000-0000000d2004', 'meeting', NULL,
  'meeting:00000000-0000-7000-8000-0000000e2002#0', '2026-11-02T15:00:00Z', 'Ada, Grace', 'All',
  'Ada: Hello.', '{"meetingId": "00000000-0000-7000-8000-0000000e2002", "startMs": 0,
  "endMs": 0, "clip": null}', '{"meetingId": "00000000-0000-7000-8000-0000000e2002",
  "segmentIndex": 0, "summary": ""}', '0199a3c4-0000-7000-8000-00000000ada0',
  '2026-11-02T16:00:00Z'),
 ('00000000-0000-7000-8000-0000000d2005', 'meeting', NULL,
  'meeting:00000000-0000-7000-8000-0000000e2003#0', '2026-11-03T15:00:00Z', '', 'Lost',
  'Lost.', '{"meetingId": "00000000-0000-7000-8000-0000000e2003", "startMs": 0,
  "endMs": 60000, "clip": null}', '{"meetingId": "00000000-0000-7000-8000-0000000e2003",
  "segmentIndex": 0, "summary": ""}', '0199a3c4-0000-7000-8000-00000000ada0',
  '2026-11-03T16:00:00Z'),
 ('00000000-0000-7000-8000-0000000d2006', 'slack', NULL, 'T1/C1/1', '2026-11-01T15:00:00Z',
  'Ada', 'Launch', 'Friday?', NULL, '{}', '0199a3c4-0000-7000-8000-00000000ada0',
  '2026-11-01T15:00:00Z');

INSERT INTO timeline_record_projects (timeline_record_id, project_id, confidence, attributed_by)
VALUES
 ('00000000-0000-7000-8000-0000000d2001', '00000000-0000-7000-8000-0000000b2001', 1, 'person'),
 ('00000000-0000-7000-8000-0000000d2002', '00000000-0000-7000-8000-0000000b2001', 0.8, 'elysia');
INSERT INTO timeline_record_events (id, timeline_record_id, project_id, kind, created_by, data,
                                    created_at)
VALUES
 ('00000000-0000-7000-8000-000000072001', '00000000-0000-7000-8000-0000000d2001',
  '00000000-0000-7000-8000-0000000b2001', 'attributed', '00000000-0000-0000-0000-000000000002',
  '{"before": null, "after": {"confidence": 0.6, "attributedBy": "elysia"}}',
  '2026-11-01T16:00:00Z'),
 ('00000000-0000-7000-8000-000000072002', '00000000-0000-7000-8000-0000000d2001',
  '00000000-0000-7000-8000-0000000b2001', 'attributed', '0199a3c4-0000-7000-8000-00000000ada0',
  '{"before": {"confidence": 0.6, "attributedBy": "elysia"},
    "after": {"confidence": 1, "attributedBy": "person"}}', '2026-11-01T17:00:00Z');

INSERT INTO knowledge_documents (id, source, timeline_record_id, title, occurred_at,
                                 content_sha256, created_by)
VALUES
 ('00000000-0000-7000-8000-000000062001', 'meeting', '00000000-0000-7000-8000-0000000d2002',
  'Launch', '2026-11-01T15:01:00Z', repeat('e', 64), '0199a3c4-0000-7000-8000-00000000ada0');
INSERT INTO knowledge_passages (document_id, position, content) VALUES
 ('00000000-0000-7000-8000-000000062001', 0, 'Launch Friday.');
"#;

#[derive(QueryableByName)]
struct Rows {
    #[diesel(sql_type = Jsonb)]
    rows: Value,
}

#[derive(Debug, QueryableByName)]
struct ExcerptRow {
    #[diesel(sql_type = SqlUuid)]
    record_id: Uuid,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    start_word_id: Option<Uuid>,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    end_word_id: Option<Uuid>,
    #[diesel(sql_type = Nullable<Text>)]
    kind: Option<String>,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    created_by: Option<Uuid>,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    meeting_id: Option<Uuid>,
}

/// How many migrations come after [`EXCERPTS`], plus [`EXCERPTS`] itself, so reverting that many
/// lands just before it however many land after it.
fn from_the_excerpts() -> u32 {
    let names: Vec<String> =
        diesel::migration::MigrationSource::<diesel::pg::Pg>::migrations(&MIGRATIONS)
            .expect("embedded migrations load")
            .iter()
            .map(|migration| migration.name().to_string())
            .collect();
    let position = names
        .iter()
        .position(|name| name == EXCERPTS)
        .expect("the migration is embedded");
    u32::try_from(names.len() - position).expect("a handful of migrations")
}

async fn up(url: &SecretString) {
    migrations::execute(url.clone(), MigrationCommand::Run)
        .await
        .expect("the excerpts apply");
}

async fn down(url: &SecretString) {
    let revert = MigrationCommand::Revert {
        count: from_the_excerpts(),
    };
    migrations::execute(url.clone(), revert)
        .await
        .expect("the excerpts roll back");
}

/// A database holding [`BEFORE_THE_EXCERPTS`], just before the excerpts, and a connection to it.
async fn laid_out_as_production() -> (SecretString, AsyncPgConnection) {
    let url = empty_database().await;
    up(&url).await;
    down(&url).await;
    let mut connection = AsyncPgConnection::establish(url.expose_secret())
        .await
        .expect("connects");
    connection
        .batch_execute(BEFORE_THE_EXCERPTS)
        .await
        .expect("the layout is stored");
    (url, connection)
}

/// Every row of `query`, whole and in one order, to compare two moments by.
async fn rows_of(connection: &mut AsyncPgConnection, query: &str) -> Value {
    diesel::sql_query(format!(
        "SELECT coalesce(jsonb_agg(row ORDER BY row::text), '[]') AS rows \
         FROM (SELECT to_jsonb(t) AS row FROM ({query}) t) rows"
    ))
    .get_result::<Rows>(connection)
    .await
    .unwrap_or_else(|error| panic!("{query}: {error}"))
    .rows
}

/// Everything up must keep and down must hand back, by table, as it read before the excerpts.
async fn kept_as_they_were(connection: &mut AsyncPgConnection) -> Vec<Value> {
    let mut tables = Vec::new();
    for query in [
        "SELECT id, source_kind, connector_id, external_id, occurred_at, actor, title, body, \
         media, metadata, action_item_id, created_by, created_at FROM timeline_records",
        "SELECT * FROM timeline_record_projects",
        "SELECT * FROM timeline_record_events",
        "SELECT id, source, timeline_record_id, title, content_sha256 FROM knowledge_documents",
        "SELECT * FROM knowledge_passages",
        "SELECT * FROM meeting_words",
        "SELECT * FROM meeting_events",
        "SELECT * FROM meetings",
    ] {
        tables.push(rows_of(connection, query).await);
    }
    tables
}

/// Each record's excerpt as up made it: its mark's range, kind, author, and meeting.
async fn excerpts(connection: &mut AsyncPgConnection) -> HashMap<Uuid, ExcerptRow> {
    diesel::sql_query(
        "SELECT records.id AS record_id, marks.start_word_id, marks.end_word_id, marks.kind, \
                marks.created_by, marks.meeting_id \
         FROM timeline_records records \
         LEFT JOIN meeting_marks marks ON marks.id = records.excerpt_mark_id",
    )
    .load::<ExcerptRow>(connection)
    .await
    .expect("reads")
    .into_iter()
    .map(|row| (row.record_id, row))
    .collect()
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn every_topic_becomes_an_excerpt_over_its_words_keeping_its_record() {
    let (url, mut connection) = laid_out_as_production().await;
    let before = kept_as_they_were(&mut connection).await;

    up(&url).await;

    assert_eq!(
        kept_as_they_were(&mut connection).await,
        before,
        "every record, attribution, history entry, document, word, and meeting stands as it was"
    );
    let made = excerpts(&mut connection).await;
    let range = |record: Uuid| {
        let row = &made[&record];
        (row.start_word_id, row.end_word_id)
    };
    // From the word standing in place of the edited one, as the edit's marks would.
    assert_eq!(range(INTROS), (Some(HELLO), Some(EVERYONE)));
    assert_ne!(range(INTROS).0, Some(HI), "a replaced word anchors nothing");
    assert_eq!(range(LAUNCH), (Some(LAUNCH_WORD), Some(FRIDAY)));
    assert_eq!(
        range(SILENCE),
        (Some(FRIDAY), Some(FRIDAY)),
        "a topic no word starts in covers the word said nearest its start"
    );
    assert_eq!(
        range(SPOKEN),
        (Some(SPOKEN_FIRST), Some(SPOKEN_LAST)),
        "a meeting in words alone's topic covers it whole"
    );
    assert_eq!(
        range(UNANCHORED),
        (None, None),
        "a meeting with no words anchors no excerpt"
    );
    assert_eq!(range(MESSAGE), (None, None), "a message is no excerpt");
    for record in [INTROS, LAUNCH, SILENCE, SPOKEN] {
        let row = &made[&record];
        assert_eq!(row.kind.as_deref(), Some("excerpt"));
        assert_eq!(
            row.created_by,
            Some(ELYSIA_USER_ID),
            "segmenting was Elysia's"
        );
    }
    assert_eq!(
        made[&INTROS].meeting_id,
        Some(ROADMAP.parse().expect("an id"))
    );
    assert_eq!(
        made[&SPOKEN].meeting_id,
        Some(UNTIMED.parse().expect("an id"))
    );
    assert_eq!(
        rows_of(
            &mut connection,
            &format!("SELECT count(*) AS marks FROM meeting_marks WHERE meeting_id = '{WORDLESS}'")
        )
        .await,
        json!([{ "marks": 0 }])
    );
    assert_eq!(
        rows_of(
            &mut connection,
            "SELECT source, through_id, finished_at FROM knowledge_backfills \
             WHERE source = 'meeting'"
        )
        .await,
        json!([{ "source": "meeting", "through_id": null, "finished_at": null }]),
        "the meetings' transcripts wait for the backfill"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn down_hands_back_every_row_exactly_and_up_applies_again() {
    let (url, mut connection) = laid_out_as_production().await;
    let before = kept_as_they_were(&mut connection).await;
    let marks_before = rows_of(&mut connection, "SELECT * FROM meeting_marks").await;

    up(&url).await;
    let first = excerpts(&mut connection).await;
    down(&url).await;
    assert_eq!(kept_as_they_were(&mut connection).await, before);
    assert_eq!(
        rows_of(&mut connection, "SELECT * FROM meeting_marks").await,
        marks_before,
        "the marks that made topics excerpts are gone"
    );

    up(&url).await;
    let again = excerpts(&mut connection).await;
    let ranges = |made: &HashMap<Uuid, ExcerptRow>| {
        made.iter()
            .map(|(record, row)| (*record, (row.start_word_id, row.end_word_id)))
            .collect::<HashMap<_, _>>()
    };
    assert_eq!(ranges(&again), ranges(&first));
}
