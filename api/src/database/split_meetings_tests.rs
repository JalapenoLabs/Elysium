// Copyright © 2026 Jalapeno Labs

//! The migration that splits meetings into captures (#231), against meetings laid out the way
//! production holds them before: corrected from their source's lines and from Whisper's, segmented
//! before correction existed, waiting on a run, and kept after their connector was removed.
//!
//! Up must give every meeting one capture holding what it was fetched with, move each Whisper
//! transcript onto its capture, and build each corrected or segmented meeting's final transcript
//! word for word as `crate::meetings::final_transcript` builds one. Down must hand every row back
//! exactly, and up must apply again.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs it.

use std::collections::HashMap;

use diesel::QueryableByName;
use diesel::sql_types::{BigInt, Jsonb, Nullable, Text, Uuid as SqlUuid};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl, SimpleAsyncConnection};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use uuid::{Uuid, uuid};

use super::migrations::{self, MIGRATIONS, MigrationCommand};
use crate::meetings::correction;
use crate::meetings::final_transcript;
use crate::meetings::source::TranscriptLine;
use crate::models::meeting_word::{BuiltWord, POSITION_GAP};
use crate::models::user::SYSTEM_USER_ID;
use crate::test_support::empty_database;
use crate::whisper::WhisperTranscript;

/// The migration under test.
const SPLIT: &str = "2026-11-10-020000-0000_split_meetings_into_captures";

/// Its connector's, corrected from the source's lines (the first fixed), with a recording, a
/// Whisper transcript, and its segments on the timeline.
const CORRECTED: Uuid = uuid!("00000000-0000-7000-8000-0000000e1001");
/// Segmented before correction existed, from the source's lines, one of them oddly spaced, one
/// empty, and one timed backwards.
const SEGMENTED_UNCORRECTED: Uuid = uuid!("00000000-0000-7000-8000-0000000e1002");
/// A source that sent no lines: corrected from Whisper's segments, the second fixed.
const WHISPER_CORRECTED: Uuid = uuid!("00000000-0000-7000-8000-0000000e1003");
/// A source that sent no lines, segmented from Whisper's segments without a correction.
const WHISPER_UNCORRECTED: Uuid = uuid!("00000000-0000-7000-8000-0000000e1004");
/// Fetched and waiting for its correction: its run builds its transcript.
const WAITING: Uuid = uuid!("00000000-0000-7000-8000-0000000e1005");
/// Listed, never fetched, and kept after its connector was removed.
const PENDING: Uuid = uuid!("00000000-0000-7000-8000-0000000e1006");

const ADA: &str = "0199a3c4-0000-7000-8000-00000000ada0";
const CONNECTOR: Uuid = uuid!("00000000-0000-7000-8000-0000000fc101");
const LOCATION: Uuid = uuid!("00000000-0000-7000-8000-00000000510a");

/// What production holds before the split.
const BEFORE_THE_SPLIT: &str = r#"
INSERT INTO users (id, kratos_identity_id, email, name, role, status, approved_at) VALUES
 ('0199a3c4-0000-7000-8000-00000000ada0', '0199a3c4-0000-7000-8000-00000000ada1',
  'ada@example.com', 'Ada', 'admin', 'active', now());
INSERT INTO storage_locations (id, name, kind, bunny_zone, bunny_region, storage_limit_bytes,
                               access_key_encrypted, created_by)
VALUES ('00000000-0000-7000-8000-00000000510a', 'Recordings', 'bunny', 'recordings', 'london',
        NULL, decode(repeat('cd', 60), 'hex'), '0199a3c4-0000-7000-8000-00000000ada0');
INSERT INTO token_accounts (id, kind, name, credential_encrypted, site_url, created_by) VALUES
 ('00000000-0000-7000-8000-0000000f0101', 'fireflies', 'Fireflies',
  decode(repeat('ab', 60), 'hex'), NULL, '0199a3c4-0000-7000-8000-00000000ada0');
INSERT INTO connectors (id, kind, token_account_id, storage_location_id, created_by) VALUES
 ('00000000-0000-7000-8000-0000000fc101', 'fireflies', '00000000-0000-7000-8000-0000000f0101',
  '00000000-0000-7000-8000-00000000510a', '0199a3c4-0000-7000-8000-00000000ada0');

INSERT INTO meetings (id, connector_id, source_kind, external_id, title, started_at, duration_ms,
                      organizer, participants, url, transcript, transcript_hash,
                      storage_location_id, recording_path, recording_content_type, segments,
                      state, created_by, created_at, updated_at)
VALUES
 ('00000000-0000-7000-8000-0000000e1001', '00000000-0000-7000-8000-0000000fc101', 'fireflies',
  'corrected', 'Roadmap', '2026-11-01T15:00:00Z', 1800000, 'ada@example.com',
  '["ada@example.com", "grace@example.com"]', 'https://app.fireflies.ai/view/corrected',
  '[{"startMs": 1000, "endMs": 4000, "speaker": "Ada", "text": "Jon ships Friday."},
    {"startMs": 4000, "endMs": 5000, "speaker": "Grace", "text": "Agreed."}]',
  repeat('a', 64), '00000000-0000-7000-8000-00000000510a',
  'meetings/00000000-0000-7000-8000-0000000e1001/recording.mp4', 'video/mp4',
  '[{"startMs": 0, "endMs": 60000, "title": "Launch", "summary": "", "projects": []}]',
  'done', '0199a3c4-0000-7000-8000-00000000ada0', '2026-11-01T16:00:00Z',
  '2026-11-01T16:30:00Z'),
 ('00000000-0000-7000-8000-0000000e1002', '00000000-0000-7000-8000-0000000fc101', 'fireflies',
  'segmented', 'Standup', '2026-11-02T15:00:00Z', NULL, '', '[]', NULL,
  '[{"startMs": 0, "endMs": 1000, "speaker": "Ada", "text": "  Morning,\tall  "},
    {"startMs": 1000, "endMs": 2000, "speaker": "Grace", "text": "   "},
    {"startMs": 3000, "endMs": 2500, "speaker": "Grace", "text": "Late again"}]',
  NULL, NULL, NULL, NULL, '[]', 'done', '0199a3c4-0000-7000-8000-00000000ada0',
  '2026-11-02T16:00:00Z', '2026-11-02T16:05:00Z'),
 ('00000000-0000-7000-8000-0000000e1003', '00000000-0000-7000-8000-0000000fc101', 'fireflies',
  'whisper-corrected', 'Design review', '2026-11-03T15:00:00Z', 60000, '', '[]', NULL, '[]',
  repeat('b', 64), '00000000-0000-7000-8000-00000000510a',
  'meetings/00000000-0000-7000-8000-0000000e1003/recording.mp3', 'audio/mpeg', NULL,
  'recorded', '0199a3c4-0000-7000-8000-00000000ada0', '2026-11-03T16:00:00Z',
  '2026-11-03T16:01:00Z'),
 ('00000000-0000-7000-8000-0000000e1004', '00000000-0000-7000-8000-0000000fc101', 'fireflies',
  'whisper-uncorrected', 'Retro', '2026-11-04T15:00:00Z', NULL, '', '[]', NULL, '[]',
  repeat('c', 64), '00000000-0000-7000-8000-00000000510a',
  'meetings/00000000-0000-7000-8000-0000000e1004/recording.mp3', 'audio/mpeg', '[]', 'segmented',
  '0199a3c4-0000-7000-8000-00000000ada0', '2026-11-04T16:00:00Z', '2026-11-04T16:01:00Z'),
 ('00000000-0000-7000-8000-0000000e1005', '00000000-0000-7000-8000-0000000fc101', 'fireflies',
  'waiting', 'Planning', '2026-11-05T15:00:00Z', NULL, '', '[]', NULL,
  '[{"startMs": 0, "endMs": 1000, "speaker": "Ada", "text": "Soon."}]', repeat('d', 64), NULL,
  NULL, NULL, NULL, 'recorded', '0199a3c4-0000-7000-8000-00000000ada0', '2026-11-05T16:00:00Z',
  '2026-11-05T16:00:00Z'),
 ('00000000-0000-7000-8000-0000000e1006', NULL, 'fireflies', 'pending', 'Kickoff',
  '2026-11-06T15:00:00Z', NULL, '', '[]', NULL, '[]', NULL, NULL, NULL, NULL, NULL, 'pending',
  '0199a3c4-0000-7000-8000-00000000ada0', '2026-11-06T16:00:00Z', '2026-11-06T16:00:00Z');

INSERT INTO meeting_transcriptions (meeting_id, transcript, transcript_hash, created_at) VALUES
 ('00000000-0000-7000-8000-0000000e1001',
  '{"model": "Whisper large-v3", "language": "en", "languageProbability": 0.99,
    "durationMs": 5000, "segments": [{"startMs": 900, "endMs": 4100,
    "text": "John ships Friday.", "words": []}]}',
  repeat('e', 64), '2026-11-01T16:10:00Z'),
 ('00000000-0000-7000-8000-0000000e1003',
  '{"model": "Whisper large-v3", "language": "en", "languageProbability": 0.99,
    "durationMs": 9000, "segments": [
      {"startMs": 0, "endMs": 2000, "text": " Welcome back. ", "words": []},
      {"startMs": 2000, "endMs": 3000, "text": "  ", "words": []},
      {"startMs": 3000, "endMs": 9000, "text": "Ely see um ships now", "words": []}]}',
  repeat('f', 64), '2026-11-03T16:10:00Z'),
 ('00000000-0000-7000-8000-0000000e1004',
  '{"model": "Whisper large-v3", "language": "en", "languageProbability": 0.99,
    "durationMs": 4000, "segments": [
      {"startMs": 500, "endMs": 4000, "text": "What went well", "words": []}]}',
  repeat('1', 64), '2026-11-04T16:10:00Z');

INSERT INTO meeting_corrections (meeting_id, transcript, transcript_hash, based_on,
                                 source_transcript_hash, whisper_transcript_hash, changed_lines,
                                 fixes, approved_by, approved_at, created_at)
VALUES
 ('00000000-0000-7000-8000-0000000e1001',
  '[{"startMs": 1000, "endMs": 4000, "speaker": "Ada", "text": "John ships Friday."},
    {"startMs": 4000, "endMs": 5000, "speaker": "Grace", "text": "Agreed."}]',
  repeat('2', 64), 'source', repeat('a', 64), repeat('e', 64), 1,
  '[{"heard": "Jon", "written": "John"}]', '0199a3c4-0000-7000-8000-00000000ada0',
  '2026-11-01T16:20:00Z', '2026-11-01T16:15:00Z'),
 ('00000000-0000-7000-8000-0000000e1003',
  '[{"startMs": 0, "endMs": 2000, "speaker": "", "text": "Welcome back."},
    {"startMs": 3000, "endMs": 9000, "speaker": "", "text": "Elysium ships now"}]',
  repeat('3', 64), 'whisper', repeat('b', 64), repeat('f', 64), 1,
  '[{"heard": "Ely see um", "written": "Elysium"}]', NULL, NULL, '2026-11-03T16:15:00Z');
"#;

#[derive(QueryableByName)]
struct Rows {
    #[diesel(sql_type = Jsonb)]
    rows: Value,
}

/// A capture as the test reads it back.
#[derive(Debug, QueryableByName)]
struct CaptureRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = SqlUuid)]
    meeting_id: Uuid,
    #[diesel(sql_type = Jsonb)]
    capture: Value,
}

/// A word as the test reads it back, in its meeting's order.
#[derive(Debug, QueryableByName)]
struct WordRow {
    #[diesel(sql_type = SqlUuid)]
    meeting_id: Uuid,
    #[diesel(sql_type = BigInt)]
    position: i64,
    #[diesel(sql_type = Text)]
    text: String,
    #[diesel(sql_type = Text)]
    speaker: String,
    #[diesel(sql_type = BigInt)]
    start_ms: i64,
    #[diesel(sql_type = BigInt)]
    end_ms: i64,
    #[diesel(sql_type = Text)]
    origin: String,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    capture_id: Option<Uuid>,
    #[diesel(sql_type = SqlUuid)]
    introduced_by: Uuid,
}

#[derive(Debug, QueryableByName)]
struct EventRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = SqlUuid)]
    meeting_id: Uuid,
    #[diesel(sql_type = Text)]
    kind: String,
    #[diesel(sql_type = SqlUuid)]
    created_by: Uuid,
    #[diesel(sql_type = Jsonb)]
    data: Value,
}

/// How many migrations come after [`SPLIT`], plus [`SPLIT`] itself, so reverting that many lands
/// just before it however many land after it.
fn from_the_split() -> u32 {
    let names: Vec<String> =
        diesel::migration::MigrationSource::<diesel::pg::Pg>::migrations(&MIGRATIONS)
            .expect("embedded migrations load")
            .iter()
            .map(|migration| migration.name().to_string())
            .collect();
    let position = names
        .iter()
        .position(|name| name == SPLIT)
        .expect("the migration is embedded");
    u32::try_from(names.len() - position).expect("a handful of migrations")
}

async fn up(url: &SecretString) {
    migrations::execute(url.clone(), MigrationCommand::Run)
        .await
        .expect("the split applies");
}

async fn down(url: &SecretString) {
    let revert = MigrationCommand::Revert {
        count: from_the_split(),
    };
    migrations::execute(url.clone(), revert)
        .await
        .expect("the split rolls back");
}

/// A database holding [`BEFORE_THE_SPLIT`], just before the split, and a connection to it.
async fn laid_out_as_production() -> (SecretString, AsyncPgConnection) {
    let url = empty_database().await;
    up(&url).await;
    down(&url).await;
    let mut connection = AsyncPgConnection::establish(url.expose_secret())
        .await
        .expect("connects");
    connection
        .batch_execute(BEFORE_THE_SPLIT)
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

/// Everything the split must hand back, by table.
async fn meetings_as_they_were(connection: &mut AsyncPgConnection) -> Vec<Value> {
    let mut tables = Vec::new();
    for table in ["meetings", "meeting_transcriptions", "meeting_corrections"] {
        tables.push(rows_of(connection, &format!("SELECT * FROM {table}")).await);
    }
    tables
}

async fn captures(connection: &mut AsyncPgConnection) -> HashMap<Uuid, CaptureRow> {
    let rows: Vec<CaptureRow> = diesel::sql_query(
        "SELECT id, meeting_id, to_jsonb(captures) - 'id' - 'meeting_id' AS capture \
         FROM meeting_captures captures",
    )
    .load(connection)
    .await
    .expect("captures load");
    rows.into_iter()
        .map(|capture| (capture.meeting_id, capture))
        .collect()
}

async fn words(connection: &mut AsyncPgConnection) -> HashMap<Uuid, Vec<WordRow>> {
    let rows: Vec<WordRow> = diesel::sql_query(
        "SELECT meeting_id, position, text, speaker, start_ms, end_ms, origin, capture_id, \
                introduced_by \
         FROM meeting_words WHERE superseded_by IS NULL ORDER BY meeting_id, position",
    )
    .load(connection)
    .await
    .expect("words load");
    let mut by_meeting: HashMap<Uuid, Vec<WordRow>> = HashMap::new();
    for word in rows {
        by_meeting.entry(word.meeting_id).or_default().push(word);
    }
    by_meeting
}

async fn builds(connection: &mut AsyncPgConnection) -> HashMap<Uuid, EventRow> {
    let rows: Vec<EventRow> =
        diesel::sql_query("SELECT id, meeting_id, kind, created_by, data FROM meeting_events")
            .load(connection)
            .await
            .expect("events load");
    rows.into_iter()
        .map(|event| (event.meeting_id, event))
        .collect()
}

/// What `crate::meetings::final_transcript` builds for meeting `meeting_id`, as it was laid out
/// before the split, from the same rows the migration read.
async fn built_in_rust(connection: &mut AsyncPgConnection, meeting_id: Uuid) -> Vec<BuiltWord> {
    let laid_out = rows_of(
        connection,
        &format!(
            "SELECT captures.transcript AS source, transcriptions.transcript AS whisper, \
                    corrections.transcript AS corrected \
             FROM meeting_captures captures \
             LEFT JOIN meeting_transcriptions transcriptions \
                 ON transcriptions.capture_id = captures.id \
             LEFT JOIN meeting_corrections corrections \
                 ON corrections.meeting_id = captures.meeting_id \
             WHERE captures.meeting_id = '{meeting_id}'"
        ),
    )
    .await;
    let row = &laid_out[0];
    let source: Vec<TranscriptLine> =
        serde_json::from_value(row["source"].clone()).expect("source lines");
    let whisper: Option<WhisperTranscript> =
        serde_json::from_value(row["whisper"].clone()).expect("a Whisper transcript, or none");
    let corrected: Option<Vec<TranscriptLine>> =
        serde_json::from_value(row["corrected"].clone()).expect("a corrected copy, or none");
    let heard = correction::base(source, whisper.as_ref()).expect("something was heard");
    let (_built_from, lines) = final_transcript::lines_to_build(&heard, corrected.as_deref());
    final_transcript::words_of(lines, 0)
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn every_meeting_becomes_a_meeting_with_one_capture_holding_what_it_was_fetched_with() {
    let (url, mut connection) = laid_out_as_production().await;
    let before = rows_of(&mut connection, "SELECT * FROM meetings").await;
    up(&url).await;

    let captures = captures(&mut connection).await;
    assert_eq!(captures.len(), 6, "one capture per meeting");
    for meeting in before.as_array().expect("rows") {
        let meeting_id: Uuid =
            serde_json::from_value(meeting["id"].clone()).expect("a meeting's id");
        let capture = &captures[&meeting_id].capture;
        for field in [
            "connector_id",
            "source_kind",
            "external_id",
            "url",
            "transcript",
            "transcript_hash",
            "storage_location_id",
            "recording_path",
            "recording_content_type",
            "created_by",
            "created_at",
            "updated_at",
        ] {
            assert_eq!(
                capture[field], meeting[field],
                "{meeting_id}'s capture keeps its {field}"
            );
        }
        assert_eq!(capture["offset_ms"], json!(0), "on the meeting's clock");
        assert_eq!(
            capture["recording_hash"],
            Value::Null,
            "a recording kept before captures is never downloaded again to hash it"
        );
    }
    assert_eq!(
        captures[&CORRECTED].capture["connector_id"],
        json!(CONNECTOR)
    );
    assert_eq!(
        captures[&CORRECTED].capture["storage_location_id"],
        json!(LOCATION)
    );
    assert_eq!(captures[&PENDING].capture["connector_id"], Value::Null);

    // Each Whisper transcript is its capture's now.
    let transcribed = rows_of(
        &mut connection,
        "SELECT captures.meeting_id, transcriptions.transcript_hash \
         FROM meeting_transcriptions transcriptions \
         JOIN meeting_captures captures ON captures.id = transcriptions.capture_id",
    )
    .await;
    assert_eq!(
        transcribed,
        json!([
            { "meeting_id": CORRECTED, "transcript_hash": "e".repeat(64) },
            { "meeting_id": WHISPER_CORRECTED, "transcript_hash": "f".repeat(64) },
            { "meeting_id": WHISPER_UNCORRECTED, "transcript_hash": "1".repeat(64) },
        ])
    );

    // The meeting keeps the event's own fields, and its connector is its capture's alone.
    let kept = rows_of(
        &mut connection,
        &format!("SELECT * FROM meetings WHERE id = '{CORRECTED}'"),
    )
    .await;
    let kept = &kept[0];
    assert_eq!(kept["title"], json!("Roadmap"));
    assert_eq!(kept["state"], json!("done"));
    assert_eq!(kept["updated_at"], json!("2026-11-01T16:30:00+00:00"));
    assert!(kept.get("connector_id").is_none());
    assert!(kept.get("transcript").is_none());
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "every meeting of the layout, read word for word against what the code builds"
)]
async fn each_final_transcript_is_built_from_its_corrected_copy_or_what_it_was_segmented_from() {
    let (url, mut connection) = laid_out_as_production().await;
    up(&url).await;
    let captures = captures(&mut connection).await;
    let words = words(&mut connection).await;
    let builds = builds(&mut connection).await;

    let read = |meeting_id: Uuid| -> Vec<(&str, &str)> {
        words[&meeting_id]
            .iter()
            .map(|word| (word.text.as_str(), word.origin.as_str()))
            .collect()
    };
    assert_eq!(
        read(CORRECTED),
        vec![
            ("John", "correction"),
            ("ships", "correction"),
            ("Friday.", "correction"),
            ("Agreed.", "source"),
        ]
    );
    assert_eq!(
        read(SEGMENTED_UNCORRECTED),
        vec![
            ("Morning,", "source"),
            ("all", "source"),
            ("Late", "source"),
            ("again", "source"),
        ]
    );
    assert_eq!(
        read(WHISPER_CORRECTED),
        vec![
            ("Welcome", "whisper"),
            ("back.", "whisper"),
            ("Elysium", "correction"),
            ("ships", "correction"),
            ("now", "correction"),
        ]
    );
    assert_eq!(
        read(WHISPER_UNCORRECTED),
        vec![
            ("What", "whisper"),
            ("went", "whisper"),
            ("well", "whisper")
        ]
    );
    // A meeting not yet corrected or segmented is built by its run, as a new one is.
    assert!(!words.contains_key(&WAITING));
    assert!(!words.contains_key(&PENDING));

    for (meeting_id, built_from) in [
        (CORRECTED, "correction"),
        (SEGMENTED_UNCORRECTED, "source"),
        (WHISPER_CORRECTED, "correction"),
        (WHISPER_UNCORRECTED, "whisper"),
    ] {
        let build = &builds[&meeting_id];
        let capture_id = captures[&meeting_id].id;
        assert_eq!(
            (build.kind.as_str(), build.created_by),
            ("built", SYSTEM_USER_ID)
        );
        assert_eq!(
            build.data,
            json!({
                "captureId": capture_id,
                "builtFrom": built_from,
                "words": words[&meeting_id].len(),
            })
        );
        // Word for word, time for time, what the code builds from the same rows.
        let expected = built_in_rust(&mut connection, meeting_id).await;
        let migrated: Vec<BuiltWord> = words[&meeting_id]
            .iter()
            .map(|word| BuiltWord {
                text: word.text.clone(),
                speaker: word.speaker.clone(),
                start_ms: word.start_ms,
                end_ms: word.end_ms,
                origin: serde_json::from_value(json!(word.origin)).expect("an origin"),
            })
            .collect();
        assert_eq!(
            migrated, expected,
            "{meeting_id} is built as the code builds it"
        );
        for (word, number) in words[&meeting_id].iter().zip(1_i64..) {
            assert_eq!(word.position, number * POSITION_GAP);
            assert_eq!(word.capture_id, Some(capture_id));
            assert_eq!(word.introduced_by, build.id);
        }
    }
    assert_eq!(
        builds.len(),
        4,
        "one build per meeting built, and no other change"
    );

    let corrected = &words[&CORRECTED];
    assert_eq!(
        corrected
            .iter()
            .map(|word| (word.speaker.as_str(), word.start_ms, word.end_ms))
            .collect::<Vec<_>>(),
        vec![
            ("Ada", 1000, 2000),
            ("Ada", 2000, 3000),
            ("Ada", 3000, 4000),
            ("Grace", 4000, 5000),
        ]
    );
    // A line timed backwards gives its words no time, rather than failing the split.
    let late = &words[&SEGMENTED_UNCORRECTED][2];
    assert_eq!((late.start_ms, late.end_ms), (3000, 3000));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn down_hands_back_every_row_exactly_and_up_applies_again() {
    let (url, mut connection) = laid_out_as_production().await;
    let before = meetings_as_they_were(&mut connection).await;

    up(&url).await;
    let first_words = words(&mut connection).await;
    down(&url).await;
    assert_eq!(meetings_as_they_were(&mut connection).await, before);

    up(&url).await;
    let again = words(&mut connection).await;
    assert_eq!(
        again
            .iter()
            .map(|(meeting_id, words)| (*meeting_id, words.len()))
            .collect::<HashMap<_, _>>(),
        first_words
            .iter()
            .map(|(meeting_id, words)| (*meeting_id, words.len()))
            .collect::<HashMap<_, _>>()
    );
    assert_eq!(
        rows_of(&mut connection, "SELECT id FROM meetings").await,
        rows_of(
            &mut connection,
            "SELECT meeting_id AS id FROM meeting_captures"
        )
        .await,
        "every meeting has its capture again"
    );
    // The ids of the people who wrote it are untouched too.
    assert_eq!(
        rows_of(
            &mut connection,
            "SELECT DISTINCT created_by FROM meeting_captures"
        )
        .await,
        json!([{ "created_by": ADA }])
    );
}
