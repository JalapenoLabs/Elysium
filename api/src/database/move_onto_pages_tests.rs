// Copyright © 2026 Jalapeno Labs

//! The migration that moves learnings and the company doc onto pages (#197), against data laid out
//! the way production holds it before: learnings revised, undone, deleted, and restored, one filed
//! under a workspace's own category, sections edited alone and together, proposals pending and
//! decided, and documents whose passages are already embedded.
//!
//! Up must carry all of it, every history entry still reading as it did. Down must hand every row
//! back exactly, and up must apply again. Nothing may be re-embedded.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs it.

use chrono::{DateTime, TimeZone, Utc};
use diesel::QueryableByName;
use diesel::sql_types::{Integer, Jsonb, Nullable, Text, Timestamptz, Uuid as SqlUuid};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl, SimpleAsyncConnection};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use uuid::{Uuid, uuid};

use super::migrations::{self, MIGRATIONS, MigrationCommand};
use crate::knowledge::passages;
use crate::models::agent::{Agent, Author};
use crate::models::company_doc::{self, Section};
use crate::models::knowledge_document::{self, Origin, content_sha256};
use crate::models::learning::{self, LearningContent, LearningError};
use crate::models::page::{
    self, COMPANY_PAGE_ID, LEARNINGS_PAGE_ID, PageContent, PageError, Placement, Provenance,
};
use crate::models::page_suggestion::{self, SuggestionError, SuggestionState};
use crate::models::user::{CODING_AGENT_USER_ID, SYSTEM_USER_ID};
use crate::test_support::{TEST_PERSON_ID, empty_database};

/// The migration under test.
const MOVE: &str = "2026-11-06-120000-0000_move_learnings_and_company_doc_onto_pages";

const SECOND_PERSON: Uuid = uuid!("0199a3c4-0000-7000-8000-00000000b0b0");

/// Revised three times, once by an undo, through two agents and by a second person.
const REVISED: Uuid = uuid!("0199a3c5-0000-7000-8000-000000000101");
const REVISED_CREATED: Uuid = uuid!("0199a3c5-0000-7000-8000-000000000a01");
const REVISED_BODY_AND_CATEGORY: Uuid = uuid!("0199a3c5-0000-7000-8000-000000000a02");
const REVISED_TITLE: Uuid = uuid!("0199a3c5-0000-7000-8000-000000000a03");
const REVISED_UNDO: Uuid = uuid!("0199a3c5-0000-7000-8000-000000000a04");
/// Deleted, and still deleted.
const DELETED: Uuid = uuid!("0199a3c5-0000-7000-8000-000000000102");
/// Deleted, restored, and revised since.
const RESTORED: Uuid = uuid!("0199a3c5-0000-7000-8000-000000000103");
/// Saved once, by an agent, and long enough to cut into several passages.
const LONG: Uuid = uuid!("0199a3c5-0000-7000-8000-000000000104");

/// A plain page from before the move, which the move must leave alone.
const PLAIN_PAGE: Uuid = uuid!("0199a3c5-0000-7000-8000-000000000201");

const PENDING: Uuid = uuid!("0199a3c5-0000-7000-8000-000000000c01");
const REJECTED: Uuid = uuid!("0199a3c5-0000-7000-8000-000000000c02");
const APPROVED: Uuid = uuid!("0199a3c5-0000-7000-8000-000000000c03");
const REVIEWED_THROUGH: Uuid = uuid!("0199a3c5-0000-7000-8000-000000000d01");

/// What production holds before the move. Each learning's and section's history replays to what its
/// row says, as every write the API made leaves it.
const BEFORE_THE_MOVE: &str = r#"
INSERT INTO users (id, kratos_identity_id, email, name, role, status, approved_at) VALUES
 ('0199a3c4-0000-7000-8000-00000000beef', '0199a3c4-0000-7000-8000-00000000cafe',
  'tester@example.com', 'Tester', 'admin', 'active', now()),
 ('0199a3c4-0000-7000-8000-00000000b0b0', '0199a3c4-0000-7000-8000-00000000d00d',
  'second@example.com', 'Second', 'member', 'active', now());

INSERT INTO learning_categories (key, label, description, position, created_by) VALUES
 ('rtl', 'RTL', 'Hardware design notes.', 0, '0199a3c4-0000-7000-8000-00000000beef');

INSERT INTO learnings (id, title, body, category, evidence, agent, agent_name, deleted_at,
                       created_by, created_at, updated_at) VALUES
 ('0199a3c5-0000-7000-8000-000000000101', 'Rebuild every index after a collation change',
  'Run REINDEX.', 'rtl', 'Lookups missed rows.', 'mcp:client-one', 'Claude Code', NULL,
  '0199a3c4-0000-7000-8000-00000000beef', '2026-10-01 09:00:00+00', '2026-10-04 09:00:00.123456+00'),
 ('0199a3c5-0000-7000-8000-000000000102', 'Old advice', 'Reboot it.', 'pitfall', 'It came back.',
  NULL, NULL, '2026-10-02 09:30:00+00',
  '0199a3c4-0000-7000-8000-00000000b0b0', '2026-10-02 09:00:00+00', '2026-10-02 09:30:00+00'),
 ('0199a3c5-0000-7000-8000-000000000103', 'Lint first', 'Run the linter first.', 'rtl',
  'It caught it on WAY-123.', NULL, NULL, NULL,
  '0199a3c4-0000-7000-8000-00000000beef', '2026-10-03 09:00:00+00', '2026-10-03 11:00:00+00'),
 ('0199a3c5-0000-7000-8000-000000000104', 'A long procedure',
  repeat('Stop the service first. ', 60) || E'\n\n' || repeat('Then rebuild the volume. ', 60),
  'procedure', 'It held through the restart.', 'mcp:client-two', NULL, NULL,
  '0199a3c4-0000-7000-8000-00000000beef', '2026-10-03 12:00:00+00', '2026-10-03 12:00:00+00');

INSERT INTO learning_events (id, learning_id, kind, created_by, agent, agent_name, data, created_at)
VALUES
 ('0199a3c5-0000-7000-8000-000000000a01', '0199a3c5-0000-7000-8000-000000000101', 'created',
  '0199a3c4-0000-7000-8000-00000000beef', 'mcp:client-one', 'Claude Code',
  '{"title": "Rebuild indexes after a collation change", "body": "Run REINDEX.",
    "category": "procedure", "evidence": "Lookups missed rows."}',
  '2026-10-01 09:00:00+00'),
 ('0199a3c5-0000-7000-8000-000000000a02', '0199a3c5-0000-7000-8000-000000000101', 'revised',
  '0199a3c4-0000-7000-8000-00000000b0b0', NULL, NULL,
  '{"changes": {"body": {"from": "Run REINDEX.", "to": "Run REINDEX DATABASE."},
                "category": {"from": "procedure", "to": "rtl"}}}',
  '2026-10-02 09:00:00+00'),
 ('0199a3c5-0000-7000-8000-000000000a03', '0199a3c5-0000-7000-8000-000000000101', 'revised',
  '0199a3c4-0000-7000-8000-00000000beef', 'mcp:client-two', 'Codex',
  '{"changes": {"title": {"from": "Rebuild indexes after a collation change",
                          "to": "Rebuild every index after a collation change"}}}',
  '2026-10-03 09:00:00+00'),
 ('0199a3c5-0000-7000-8000-000000000a04', '0199a3c5-0000-7000-8000-000000000101', 'revised',
  '0199a3c4-0000-7000-8000-00000000b0b0', NULL, NULL,
  '{"changes": {"body": {"from": "Run REINDEX DATABASE.", "to": "Run REINDEX."}},
    "undid": "0199a3c5-0000-7000-8000-000000000a02"}',
  '2026-10-04 09:00:00+00'),

 ('0199a3c5-0000-7000-8000-000000000a11', '0199a3c5-0000-7000-8000-000000000102', 'created',
  '0199a3c4-0000-7000-8000-00000000b0b0', NULL, NULL,
  '{"title": "Old advice", "body": "Reboot it.", "category": "pitfall",
    "evidence": "It came back."}',
  '2026-10-02 09:00:00+00'),
 ('0199a3c5-0000-7000-8000-000000000a12', '0199a3c5-0000-7000-8000-000000000102', 'deleted',
  '0199a3c4-0000-7000-8000-00000000beef', NULL, NULL, '{}', '2026-10-02 09:30:00+00'),

 ('0199a3c5-0000-7000-8000-000000000a21', '0199a3c5-0000-7000-8000-000000000103', 'created',
  '0199a3c4-0000-7000-8000-00000000beef', NULL, NULL,
  '{"title": "Lint first", "body": "Lint.", "category": "rtl",
    "evidence": "It caught it on WAY-123."}',
  '2026-10-03 09:00:00+00'),
 ('0199a3c5-0000-7000-8000-000000000a22', '0199a3c5-0000-7000-8000-000000000103', 'deleted',
  '0199a3c4-0000-7000-8000-00000000beef', NULL, NULL, '{}', '2026-10-03 09:30:00+00'),
 ('0199a3c5-0000-7000-8000-000000000a23', '0199a3c5-0000-7000-8000-000000000103', 'restored',
  '0199a3c4-0000-7000-8000-00000000b0b0', NULL, NULL,
  '{"deletedAt": "2026-10-03T09:30:00Z", "undid": "0199a3c5-0000-7000-8000-000000000a22"}',
  '2026-10-03 10:00:00+00'),
 ('0199a3c5-0000-7000-8000-000000000a24', '0199a3c5-0000-7000-8000-000000000103', 'revised',
  '0199a3c4-0000-7000-8000-00000000b0b0', NULL, NULL,
  '{"changes": {"body": {"from": "Lint.", "to": "Run the linter first."}}}',
  '2026-10-03 11:00:00+00');

INSERT INTO learning_events (id, learning_id, kind, created_by, agent, agent_name, data, created_at)
SELECT '0199a3c5-0000-7000-8000-000000000a31', id, 'created', created_by, agent, agent_name,
       jsonb_build_object('title', title, 'body', body, 'category', category, 'evidence', evidence),
       created_at
FROM learnings WHERE id = '0199a3c5-0000-7000-8000-000000000104';

INSERT INTO pages (id, position, title, body, created_by, created_at) VALUES
 ('0199a3c5-0000-7000-8000-000000000201', 1048576, 'Runbooks', 'Start here.',
  '0199a3c4-0000-7000-8000-00000000beef', '2026-10-05 09:00:00+00');
INSERT INTO page_versions (page_id, version, title, body, created_by, created_at) VALUES
 ('0199a3c5-0000-7000-8000-000000000201', 1, 'Runbooks', 'Start here.',
  '0199a3c4-0000-7000-8000-00000000beef', '2026-10-05 09:00:00+00');
INSERT INTO page_events (id, page_id, kind, created_by, data, created_at) VALUES
 ('0199a3c5-0000-7000-8000-000000000e01', '0199a3c5-0000-7000-8000-000000000201', 'created',
  '0199a3c4-0000-7000-8000-00000000beef', '{"parentId": null, "version": 1}',
  '2026-10-05 09:00:00+00');

INSERT INTO company_doc_proposals (id, section, base_content, content, reason, quote, source,
                                   state, decided_by, decided_at, created_by, agent, agent_name,
                                   created_at) VALUES
 ('0199a3c5-0000-7000-8000-000000000c01', 'glossary', '- ACME: a client',
  E'- ACME: a client\n- WAY: the tracker', 'Said in the kickoff.', 'We call it WAY.',
  'Kickoff, October 1st', 'pending', NULL, NULL,
  '0199a3c4-0000-7000-8000-00000000beef', 'mcp:client-one', 'Claude Code',
  '2026-10-06 09:00:00+00'),
 ('0199a3c5-0000-7000-8000-000000000c02', 'people', '- Priya', '- Pria', 'A typo.', NULL, NULL,
  'rejected', '0199a3c4-0000-7000-8000-00000000beef', '2026-10-06 10:00:00+00',
  '00000000-0000-0000-0000-000000000003', 'session:12', NULL, '2026-10-06 09:30:00+00'),
 ('0199a3c5-0000-7000-8000-000000000c03', 'people', '- Priya', E'- Priya Raman', 'Her full name.',
  NULL, NULL, 'approved', '0199a3c4-0000-7000-8000-00000000b0b0', '2026-10-05 12:00:00+00',
  '00000000-0000-0000-0000-000000000002', NULL, NULL, '2026-10-05 11:00:00+00');

UPDATE company_doc_sections SET content = '- Priya Raman', updated_at = '2026-10-05 12:00:00+00'
WHERE section = 'people';
UPDATE company_doc_sections SET content = '- ACME: a client', updated_at = '2026-10-05 10:00:00+00'
WHERE section = 'glossary';
UPDATE company_doc_sections SET updated_at = '2026-10-05 13:00:00+00'
WHERE section = 'products';

INSERT INTO company_doc_events (id, kind, created_by, proposal_id, data, created_at) VALUES
 ('0199a3c5-0000-7000-8000-000000000b01', 'edited', '0199a3c4-0000-7000-8000-00000000beef', NULL,
  '{"changes": {"products": {"from": "", "to": "- Elysium"}}}', '2026-10-05 09:00:00+00'),
 ('0199a3c5-0000-7000-8000-000000000b02', 'edited', '0199a3c4-0000-7000-8000-00000000beef', NULL,
  '{"changes": {"people": {"from": "", "to": "- Priya"},
                "glossary": {"from": "", "to": "- ACME: a client"}}}',
  '2026-10-05 10:00:00+00'),
 ('0199a3c5-0000-7000-8000-000000000b03', 'approved', '0199a3c4-0000-7000-8000-00000000b0b0',
  '0199a3c5-0000-7000-8000-000000000c03',
  '{"changes": {"people": {"from": "- Priya", "to": "- Priya Raman"}}}',
  '2026-10-05 12:00:00+00'),
 ('0199a3c5-0000-7000-8000-000000000b04', 'edited', '0199a3c4-0000-7000-8000-00000000b0b0', NULL,
  '{"changes": {"products": {"from": "- Elysium", "to": ""}},
    "undid": "0199a3c5-0000-7000-8000-000000000b01"}',
  '2026-10-05 13:00:00+00');

UPDATE company_doc_review_cursor
SET reviewed_through_at = '2026-10-06 08:00:00+00',
    reviewed_through_id = '0199a3c5-0000-7000-8000-000000000d01';
"#;

#[derive(QueryableByName)]
struct Rows {
    #[diesel(sql_type = Jsonb)]
    rows: Value,
}

/// A source's text as the API indexed it, before the move.
#[derive(QueryableByName)]
struct Source {
    #[diesel(sql_type = Text)]
    kind: String,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    learning_id: Option<Uuid>,
    #[diesel(sql_type = Nullable<Text>)]
    section: Option<String>,
    #[diesel(sql_type = Text)]
    title: String,
    #[diesel(sql_type = Text)]
    text: String,
    #[diesel(sql_type = Timestamptz)]
    occurred_at: DateTime<Utc>,
    #[diesel(sql_type = SqlUuid)]
    created_by: Uuid,
}

#[derive(QueryableByName)]
struct Version {
    #[diesel(sql_type = Integer)]
    number: i32,
    #[diesel(sql_type = Text)]
    title: String,
    #[diesel(sql_type = Text)]
    body: String,
    #[diesel(sql_type = Nullable<Text>)]
    category: Option<String>,
    #[diesel(sql_type = SqlUuid)]
    created_by: Uuid,
    #[diesel(sql_type = Nullable<Text>)]
    agent: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    agent_name: Option<String>,
    #[diesel(sql_type = Timestamptz)]
    created_at: DateTime<Utc>,
}

/// How many migrations come after [`MOVE`], plus [`MOVE`] itself, so reverting that many lands just
/// before it however many land after it.
fn from_the_move() -> u32 {
    let names: Vec<String> =
        diesel::migration::MigrationSource::<diesel::pg::Pg>::migrations(&MIGRATIONS)
            .expect("embedded migrations load")
            .iter()
            .map(|migration| migration.name().to_string())
            .collect();
    let position = names
        .iter()
        .position(|name| name == MOVE)
        .expect("the migration is embedded");
    u32::try_from(names.len() - position).expect("a handful of migrations")
}

async fn up(url: &SecretString) {
    migrations::execute(url.clone(), MigrationCommand::Run)
        .await
        .expect("the move applies");
}

async fn down(url: &SecretString) {
    let revert = MigrationCommand::Revert {
        count: from_the_move(),
    };
    migrations::execute(url.clone(), revert)
        .await
        .expect("the move rolls back");
}

/// Stores every active learning's and written section's document, as the API indexed them, with
/// every passage embedded.
async fn index_everything(connection: &mut AsyncPgConnection) {
    let sources: Vec<Source> = diesel::sql_query(
        "SELECT 'learning' AS kind, id AS learning_id, NULL::text AS section, title, \
                title || E'\\n\\n' || body || E'\\n\\n' || evidence AS text, \
                updated_at AS occurred_at, created_by \
         FROM learnings WHERE deleted_at IS NULL \
         UNION ALL \
         SELECT 'company_doc', NULL, section, \
                'Company doc: ' || initcap(section), content, updated_at, \
                '0199a3c4-0000-7000-8000-00000000beef' \
         FROM company_doc_sections WHERE content <> ''",
    )
    .load(connection)
    .await
    .expect("sources load");

    for source in sources {
        let cut = passages::split(&source.text);
        let document = Uuid::now_v7();
        diesel::sql_query(
            "INSERT INTO knowledge_documents (id, source, learning_id, company_doc_section, title, \
                                              occurred_at, content_sha256, created_by) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind::<SqlUuid, _>(document)
        .bind::<Text, _>(&source.kind)
        .bind::<Nullable<SqlUuid>, _>(source.learning_id)
        .bind::<Nullable<Text>, _>(&source.section)
        .bind::<Text, _>(&source.title)
        .bind::<Timestamptz, _>(source.occurred_at)
        .bind::<Text, _>(content_sha256(&cut))
        .bind::<SqlUuid, _>(source.created_by)
        .execute(connection)
        .await
        .expect("the document is stored");

        for (index, passage) in cut.iter().enumerate() {
            let position = i32::try_from(index).expect("few passages");
            // A vector of its own, so a passage given another's embedding shows.
            let embedding = format!("[{}]", vec![format!("0.{}", position + 1); 768].join(","));
            diesel::sql_query(
                "INSERT INTO knowledge_passages (document_id, position, content, embedding, \
                                                 embedding_model) \
                 VALUES ($1, $2, $3, $4::vector, 'model@revision')",
            )
            .bind::<SqlUuid, _>(document)
            .bind::<Integer, _>(position)
            .bind::<Text, _>(passage)
            .bind::<Text, _>(embedding)
            .execute(connection)
            .await
            .expect("the passage is stored");
        }
    }
}

/// A database holding [`BEFORE_THE_MOVE`], just before the move, and a connection to it.
async fn laid_out_as_production() -> (SecretString, AsyncPgConnection) {
    let url = empty_database().await;
    migrations::execute(url.clone(), MigrationCommand::Run)
        .await
        .expect("up");
    down(&url).await;
    let mut connection = AsyncPgConnection::establish(url.expose_secret())
        .await
        .expect("connects");
    connection
        .batch_execute(BEFORE_THE_MOVE)
        .await
        .expect("the layout is stored");
    index_everything(&mut connection).await;
    (url, connection)
}

/// Every row of `query`'s `t`, whole and in one order, to compare two moments by.
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

/// Everything the move must hand back, by table.
const OLD_TABLES: [&str; 8] = [
    "learnings",
    "learning_events",
    "company_doc_sections",
    "company_doc_events",
    "company_doc_proposals",
    "company_doc_review_cursor",
    "knowledge_documents",
    "knowledge_passages",
];

async fn old_tables(connection: &mut AsyncPgConnection) -> Vec<Value> {
    let mut rows = Vec::new();
    for table in OLD_TABLES {
        rows.push(rows_of(connection, &format!("SELECT * FROM {table}")).await);
    }
    rows.push(
        rows_of(
            connection,
            "SELECT * FROM pages WHERE id = '0199a3c5-0000-7000-8000-000000000201'",
        )
        .await,
    );
    rows.push(rows_of(connection, "SELECT * FROM page_versions").await);
    rows.push(rows_of(connection, "SELECT * FROM page_events").await);
    rows
}

/// What each passage holds and what embedded it, and each document's hash, which a re-embed changes.
async fn index_state(connection: &mut AsyncPgConnection) -> Value {
    rows_of(
        connection,
        "SELECT d.id, d.source, d.content_sha256, p.position, p.content, p.embedding, \
                p.embedding_model \
         FROM knowledge_documents d JOIN knowledge_passages p ON p.document_id = d.id",
    )
    .await
}

async fn versions_of(connection: &mut AsyncPgConnection, page_id: Uuid) -> Vec<Version> {
    diesel::sql_query(
        "SELECT version AS number, title, body, category, created_by, agent, agent_name, created_at \
         FROM page_versions WHERE page_id = $1 ORDER BY version",
    )
    .bind::<SqlUuid, _>(page_id)
    .load(connection)
    .await
    .expect("versions load")
}

fn at(month_day: (u32, u32), hour: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, month_day.0, month_day.1, hour, 0, 0)
        .single()
        .expect("a real instant")
}

fn person() -> Author<'static> {
    Author {
        user_id: TEST_PERSON_ID,
        agent: None,
        at: Utc::now(),
    }
}

/// The rows of a history table in order of their ids, each its id, kind, and `data`.
fn entry_rows(rows: &Value) -> Vec<(String, String, Value)> {
    let mut entries: Vec<(String, String, Value)> = rows
        .as_array()
        .expect("rows")
        .iter()
        .map(|row| {
            (
                row["id"].as_str().expect("an id").to_owned(),
                row["kind"].as_str().expect("a kind").to_owned(),
                row["data"].clone(),
            )
        })
        .collect();
    entries.sort_by(|first, second| first.0.cmp(&second.0));
    entries
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_move_carries_every_learning_with_its_revisions_as_versions() {
    let (url, mut connection) = laid_out_as_production().await;
    up(&url).await;

    let stored = page::find(&mut connection, REVISED).await.expect("kept");
    assert_eq!(stored.parent_id, Some(LEARNINGS_PAGE_ID));
    assert_eq!(stored.title, "Rebuild every index after a collation change");
    assert_eq!(stored.category.as_deref(), Some("rtl"));
    assert_eq!(stored.evidence.as_deref(), Some("Lookups missed rows."));
    assert_eq!(stored.current_version, 4);
    assert_eq!(stored.created_by, TEST_PERSON_ID);
    assert_eq!(stored.created_at, at((10, 1), 9));
    assert!(!stored.proposal_only);

    let versions = versions_of(&mut connection, REVISED).await;
    let bodies: Vec<&str> = versions
        .iter()
        .map(|version| version.body.as_str())
        .collect();
    assert_eq!(
        bodies,
        [
            "Run REINDEX.",
            "Run REINDEX DATABASE.",
            "Run REINDEX DATABASE.",
            "Run REINDEX."
        ]
    );
    let categories: Vec<_> = versions
        .iter()
        .map(|version| version.category.as_deref())
        .collect();
    assert_eq!(
        categories,
        [Some("procedure"), Some("rtl"), Some("rtl"), Some("rtl")]
    );
    assert_eq!(
        versions[0].title,
        "Rebuild indexes after a collation change"
    );
    assert_eq!(
        versions[2].title,
        "Rebuild every index after a collation change"
    );
    assert_eq!(
        versions
            .iter()
            .map(|version| version.number)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert_eq!(
        (
            versions[0].created_by,
            versions[0].agent.as_deref(),
            versions[0].agent_name.as_deref()
        ),
        (TEST_PERSON_ID, Some("mcp:client-one"), Some("Claude Code")),
        "the first save keeps who saved it and through what"
    );
    assert_eq!(
        (versions[1].created_by, versions[1].agent.as_deref()),
        (SECOND_PERSON, None),
        "each revision keeps its own author"
    );
    assert_eq!(
        (
            versions[2].agent.as_deref(),
            versions[2].agent_name.as_deref()
        ),
        (Some("mcp:client-two"), Some("Codex"))
    );
    assert_eq!(versions[3].created_at, at((10, 4), 9));

    let deleted = page::find(&mut connection, DELETED).await.expect("kept");
    assert_eq!(
        deleted.deleted_at,
        Some(at((10, 2), 9) + chrono::Duration::minutes(30))
    );

    let plain = page::find(&mut connection, PLAIN_PAGE).await.expect("kept");
    assert_eq!(plain.parent_id, None);
    assert_eq!(plain.category, None);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn every_learning_and_history_entry_reads_as_it_did() {
    let (url, mut connection) = laid_out_as_production().await;
    let learnings_before = rows_of(&mut connection, "SELECT * FROM learnings").await;
    let events_before = rows_of(&mut connection, "SELECT * FROM learning_events ORDER BY id").await;
    up(&url).await;

    let mut listed = learning::list(&mut connection, false).await.expect("lists");
    let gone = learning::list(&mut connection, true).await.expect("lists");
    assert_eq!(listed.len(), 3);
    assert_eq!(
        gone.iter().map(|learning| learning.id).collect::<Vec<_>>(),
        [DELETED]
    );
    listed.extend(gone);

    for before in learnings_before.as_array().expect("rows") {
        let id: Uuid = before["id"]
            .as_str()
            .expect("an id")
            .parse()
            .expect("a uuid");
        let learning = learning::find(&mut connection, id).await.expect("found");
        assert_eq!(json!(learning.title), before["title"]);
        assert_eq!(json!(learning.body), before["body"]);
        assert_eq!(json!(learning.category), before["category"]);
        assert_eq!(json!(learning.evidence), before["evidence"]);
        assert_eq!(json!(learning.agent), before["agent"]);
        assert_eq!(json!(learning.agent_name), before["agent_name"]);
        assert_eq!(json!(learning.created_by), before["created_by"]);
    }

    let mut read = Vec::new();
    for id in [REVISED, DELETED, RESTORED, LONG] {
        read.extend(learning::history(&mut connection, id).await.expect("reads"));
    }
    read.sort_by_key(|entry| entry.id);
    let read: Vec<(String, String, Value)> = read
        .into_iter()
        .map(|entry| (entry.id.to_string(), entry.kind, entry.data))
        .collect();
    assert_eq!(read, entry_rows(&events_before));

    let revision = learning::history(&mut connection, REVISED)
        .await
        .expect("reads")
        .into_iter()
        .find(|entry| entry.id == REVISED_TITLE)
        .expect("kept");
    assert_eq!(revision.created_by, TEST_PERSON_ID);
    assert_eq!(revision.agent.as_deref(), Some("mcp:client-two"));
    assert_eq!(revision.agent_name.as_deref(), Some("Codex"));
    assert_eq!(revision.created_at, at((10, 3), 9));

    let feed = learning::feed(&mut connection, None, 200)
        .await
        .expect("reads");
    assert_eq!(feed.len(), read.len(), "the admins' feed holds every entry");
    assert_eq!(feed[0].1.id, LONG, "newest first");
    let undo = feed.iter().find(|(entry, _)| entry.id == REVISED_UNDO);
    let (_, learning) = undo.expect("in the feed");
    assert_eq!(learning.id, REVISED);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_company_doc_reads_as_it_did_with_its_history_and_proposals() {
    let (url, mut connection) = laid_out_as_production().await;
    let events_before = rows_of(
        &mut connection,
        "SELECT * FROM company_doc_events ORDER BY id",
    )
    .await;
    up(&url).await;

    let root = page::find(&mut connection, COMPANY_PAGE_ID)
        .await
        .expect("seeded");
    assert_eq!(root.title, "Company");
    assert!(root.proposal_only, "agents only propose to the doc");

    for section in Section::ALL {
        let stored = page::find(&mut connection, section.page_id())
            .await
            .expect("seeded");
        assert_eq!(stored.parent_id, Some(COMPANY_PAGE_ID));
        assert_eq!(stored.title, section.heading());
        assert!(stored.proposal_only, "{section:?}");
    }

    let sections = company_doc::sections(&mut connection).await.expect("reads");
    assert_eq!(
        sections
            .iter()
            .map(|section| section.section)
            .collect::<Vec<_>>(),
        Section::ALL
    );
    let body_of = |wanted: Section| {
        let section = sections.iter().find(|section| section.section == wanted);
        section.expect("every section").content.as_str()
    };
    assert_eq!(body_of(Section::People), "- Priya Raman");
    assert_eq!(body_of(Section::Glossary), "- ACME: a client");
    assert_eq!(body_of(Section::Products), "");

    let people = versions_of(&mut connection, Section::People.page_id()).await;
    assert_eq!(
        people
            .iter()
            .map(|version| version.body.as_str())
            .collect::<Vec<_>>(),
        ["", "- Priya", "- Priya Raman"]
    );
    assert_eq!(
        people[0].created_by, SYSTEM_USER_ID,
        "the seed is the system's"
    );
    assert_eq!(people[2].created_by, SECOND_PERSON, "whoever approved it");

    // The doc's history is its sections' pages': every entry keeps its id on each section it wrote,
    // and the approved one names what it applied, as the suggestion that proposal now is.
    let written = rows_of(
        &mut connection,
        "SELECT id, page_id, created_by, created_at, data FROM page_events \
         WHERE kind = 'published' AND page_id IN (SELECT id FROM pages \
                                                  WHERE parent_id = '00000000-0000-0000-0001-000000000002') \
         ORDER BY id, page_id",
    )
    .await;
    let written = written.as_array().expect("rows");
    let mut entry_ids: Vec<String> = written
        .iter()
        .map(|row| row["id"].as_str().expect("an id").to_owned())
        .collect();
    entry_ids.dedup();
    let before: Vec<String> = entry_rows(&events_before)
        .into_iter()
        .map(|(id, _, _)| id)
        .collect();
    assert_eq!(entry_ids, before, "every entry keeps its id");
    let approved = written
        .iter()
        .find(|row| row["data"].get("suggestionId").is_some())
        .expect("kept");
    assert_eq!(approved["data"]["suggestionId"], json!(APPROVED));
    assert_eq!(approved["created_by"], json!(SECOND_PERSON));
    assert_eq!(approved["page_id"], json!(Section::People.page_id()));

    let pending = company_doc::pending(&mut connection).await.expect("reads");
    assert_eq!(
        pending
            .iter()
            .map(|suggested| suggested.suggestion.id)
            .collect::<Vec<_>>(),
        [PENDING]
    );
    let dismissed = company_doc::latest(&mut connection, SuggestionState::Dismissed, 10)
        .await
        .expect("reads");
    assert_eq!(
        dismissed
            .iter()
            .map(|suggested| suggested.suggestion.id)
            .collect::<Vec<_>>(),
        [REJECTED]
    );

    let cursor = company_doc::review_cursor(&mut connection)
        .await
        .expect("reads");
    assert_eq!(cursor, Some((at((10, 6), 8), REVIEWED_THROUGH)));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn no_passage_is_embedded_again_by_the_move_or_by_indexing_after_it() {
    let (url, mut connection) = laid_out_as_production().await;
    let before = index_state(&mut connection).await;
    assert!(
        before.as_array().expect("rows").len() >= 6,
        "the long learning is cut into several passages"
    );

    up(&url).await;
    assert_eq!(
        index_state(&mut connection).await,
        before,
        "the move changes no document's text, hash, or embedding"
    );

    // Indexing is what would re-embed: each page must cut into exactly what is stored.
    for id in [REVISED, RESTORED, LONG] {
        let stored = page::find(&mut connection, id).await.expect("kept");
        page::reindex(&mut connection, &stored)
            .await
            .expect("indexes");
    }
    for section in Section::ALL {
        let stored = page::find(&mut connection, section.page_id())
            .await
            .expect("kept");
        page::reindex(&mut connection, &stored)
            .await
            .expect("indexes");
    }
    assert_eq!(
        index_state(&mut connection).await,
        before,
        "a page indexes from the text its passages were embedded from"
    );
    let plain_documents = rows_of(
        &mut connection,
        "SELECT id FROM knowledge_documents WHERE source = 'page'",
    )
    .await;
    assert_eq!(
        plain_documents,
        json!([]),
        "the Learnings and Company pages say nothing yet, so they have no document"
    );

    let document_of = |source: &str, id: Uuid| {
        format!("SELECT id FROM knowledge_documents WHERE source = '{source}' AND page_id = '{id}'")
    };
    let learning_document = rows_of(&mut connection, &document_of("learning", REVISED)).await;
    let id: Uuid = learning_document[0]["id"]
        .as_str()
        .expect("indexed")
        .parse()
        .expect("a uuid");
    let opened = knowledge_document::get(&mut connection, id)
        .await
        .expect("reads")
        .expect("found");
    assert_eq!(
        opened.origin,
        Origin::Learning {
            learning_id: REVISED
        }
    );

    let section_document = rows_of(
        &mut connection,
        &document_of("company_doc", Section::People.page_id()),
    )
    .await;
    let id: Uuid = section_document[0]["id"]
        .as_str()
        .expect("indexed")
        .parse()
        .expect("a uuid");
    let opened = knowledge_document::get(&mut connection, id)
        .await
        .expect("reads")
        .expect("found");
    assert_eq!(
        opened.origin,
        Origin::CompanyDoc {
            section: "people".to_owned(),
            page_id: Section::People.page_id(),
        }
    );
    assert_eq!(opened.citation.title, "Company doc: People");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn down_hands_back_every_row_exactly_and_up_applies_again() {
    let (url, mut connection) = laid_out_as_production().await;
    let before = old_tables(&mut connection).await;

    up(&url).await;
    // The pages seeded at the top of the tree take a new instant each time: Learnings and Company
    // by this migration, and Project briefs and Transcription by later ones.
    let moved_pages = "SELECT * FROM pages WHERE id NOT IN \
                       ('00000000-0000-0000-0001-000000000001', \
                        '00000000-0000-0000-0001-000000000002', \
                        '00000000-0000-0000-0001-000000000003', \
                        '00000000-0000-0000-0001-000000000005')";
    let moved_versions = "SELECT * FROM page_versions WHERE page_id NOT IN \
                          ('00000000-0000-0000-0001-000000000001', \
                           '00000000-0000-0000-0001-000000000002', \
                           '00000000-0000-0000-0001-000000000003', \
                           '00000000-0000-0000-0001-000000000005')";
    // A section's first entry takes a new id each time; everything else keeps its own.
    let moved_events = "SELECT * FROM page_events WHERE kind <> 'created' \
                        OR page_id NOT IN (SELECT id FROM pages WHERE parent_id IS NULL \
                                           OR parent_id = '00000000-0000-0000-0001-000000000002')";
    let once = [
        rows_of(&mut connection, moved_pages).await,
        rows_of(&mut connection, moved_versions).await,
        rows_of(&mut connection, moved_events).await,
        rows_of(&mut connection, "SELECT * FROM knowledge_documents").await,
    ];

    down(&url).await;
    let after = old_tables(&mut connection).await;
    for (index, (then, now)) in before.iter().zip(&after).enumerate() {
        assert_eq!(
            now, then,
            "table {index} of the snapshot is not what it was"
        );
    }

    up(&url).await;
    let twice = [
        rows_of(&mut connection, moved_pages).await,
        rows_of(&mut connection, moved_versions).await,
        rows_of(&mut connection, moved_events).await,
        rows_of(&mut connection, "SELECT * FROM knowledge_documents").await,
    ];
    assert_eq!(twice, once, "the move lands the same way the second time");
}

/// Writes something of each kind the old tables must take back. Answers the learning saved by a
/// person, and the two pages of someone's own that sit under what leaves.
async fn write_since_the_move(
    connection: &mut AsyncPgConnection,
) -> (Uuid, [(Uuid, &'static str); 2]) {
    let saved = learning::save(
        connection,
        LearningContent {
            title: "Saved on a page".to_owned(),
            body: "After the move.".to_owned(),
            category: "convention".to_owned(),
            evidence: "It is in the tree.".to_owned(),
        },
        person(),
    )
    .await
    .expect("saved");
    // Recorded on a page, an agent no learning could name.
    let session = Agent::session(12);
    let by_a_session = Author {
        user_id: CODING_AGENT_USER_ID,
        agent: Some(&session),
        at: Utc::now(),
    };
    learning::save(
        connection,
        LearningContent {
            title: "Saved by a session".to_owned(),
            body: "After the move.".to_owned(),
            category: "convention".to_owned(),
            evidence: "It is in the tree.".to_owned(),
        },
        by_a_session,
    )
    .await
    .expect("saved");

    // A page of someone's own under the Learnings page, another under a learning, a learning
    // moved, and a learning deleted with a descendant.
    let edit = |title: &str| page::PageEdit {
        title: title.to_owned(),
        body: String::new(),
        learning: None,
    };
    let under_learnings = page::create(
        connection,
        Some(LEARNINGS_PAGE_ID),
        edit("Notes on learnings"),
        person(),
    )
    .await
    .expect("created");
    let under_a_learning = page::create(
        connection,
        Some(REVISED),
        edit("Notes on one learning"),
        person(),
    )
    .await
    .expect("created");
    let placement = Placement {
        parent_id: Some(PLAIN_PAGE),
        after: None,
    };
    page::move_page(connection, LONG, placement, person())
        .await
        .expect("moved");

    // One change to two sections, as one entry id on both, and the suggestion pending before the
    // move accepted.
    let change = Provenance {
        change: Some(Uuid::now_v7()),
        ..Provenance::default()
    };
    for (section, body) in [
        (Section::People, "- Priya Raman\n- Tom Okafor"),
        (Section::Products, "- Elysium"),
    ] {
        let current = page::lock(connection, section.page_id())
            .await
            .expect("locked");
        let content = PageContent {
            body: body.to_owned(),
            ..current.content()
        };
        page::publish_content(connection, &current, content, change, person())
            .await
            .expect("published");
    }
    page_suggestion::accept(
        connection,
        Section::Glossary.page_id(),
        PENDING,
        None,
        person(),
    )
    .await
    .expect("accepted");

    (
        saved.learning.id,
        [
            (under_learnings.page.id, "Notes on learnings"),
            (under_a_learning.page.id, "Notes on one learning"),
        ],
    )
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn down_carries_what_was_written_since_the_move() {
    let (url, mut connection) = laid_out_as_production().await;
    up(&url).await;
    let (saved, own_pages) = write_since_the_move(&mut connection).await;

    down(&url).await;

    let learnings = rows_of(&mut connection, "SELECT id, title, agent FROM learnings").await;
    assert_eq!(learnings.as_array().expect("rows").len(), 6);
    let by_session = rows_of(
        &mut connection,
        "SELECT agent FROM learnings WHERE title = 'Saved by a session'",
    )
    .await;
    assert_eq!(by_session, json!([{ "agent": null }]));

    let revised = rows_of(
        &mut connection,
        &format!(
            "SELECT kind, data FROM learning_events WHERE learning_id = '{saved}' ORDER BY id"
        ),
    )
    .await;
    assert_eq!(revised[0]["kind"], "created");
    assert_eq!(revised[0]["data"]["title"], "Saved on a page");

    for (id, title) in own_pages {
        let kept = rows_of(
            &mut connection,
            &format!("SELECT parent_id, title FROM pages WHERE id = '{id}'"),
        )
        .await;
        assert_eq!(
            kept,
            json!([{ "parent_id": null, "title": title }]),
            "a page of someone's own outlives what it sat under"
        );
    }

    let people = rows_of(
        &mut connection,
        "SELECT content FROM company_doc_sections WHERE section = 'people'",
    )
    .await;
    assert_eq!(
        people,
        json!([{ "content": "- Priya Raman\n- Tom Okafor" }])
    );
    let entries = rows_of(
        &mut connection,
        "SELECT kind, proposal_id FROM company_doc_events ORDER BY id DESC LIMIT 1",
    )
    .await;
    assert_eq!(
        entries,
        json!([{ "kind": "approved", "proposal_id": PENDING }]),
        "what was approved since is an approval again"
    );
    let two_sections = rows_of(
        &mut connection,
        "SELECT data FROM company_doc_events ORDER BY id DESC OFFSET 1 LIMIT 1",
    )
    .await;
    let changes = two_sections[0]["data"]["changes"]
        .as_object()
        .expect("changes");
    assert_eq!(
        changes.len(),
        2,
        "one change to two sections is one entry again"
    );

    up(&url).await;
    assert_eq!(
        learning::list(&mut connection, false)
            .await
            .expect("lists")
            .len(),
        5,
        "the move applies again over what was written since"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_history_that_does_not_replay_stops_the_move_and_changes_nothing() {
    let (url, mut connection) = laid_out_as_production().await;
    let before = old_tables(&mut connection).await;
    connection
        .batch_execute(
            "UPDATE learnings SET body = 'Edited by hand.' \
             WHERE id = '0199a3c5-0000-7000-8000-000000000104'",
        )
        .await
        .expect("the row drifts from its history");

    let refused = migrations::execute(url.clone(), MigrationCommand::Run).await;

    assert!(
        refused.is_err(),
        "a history that does not end on its row is not guessed at"
    );
    connection
        .batch_execute(
            "UPDATE learnings SET body = \
                (SELECT data ->> 'body' FROM learning_events \
                 WHERE learning_id = learnings.id AND kind = 'created') \
             WHERE id = '0199a3c5-0000-7000-8000-000000000104'",
        )
        .await
        .expect("the tables are all still there");
    let after = old_tables(&mut connection).await;
    assert_eq!(after[1..], before[1..], "nothing moved");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn what_was_true_of_learnings_still_holds_on_moved_data() {
    let (url, mut connection) = laid_out_as_production().await;
    up(&url).await;

    // Saving a title again, in another case, revises that learning.
    let revised = learning::save(
        &mut connection,
        LearningContent {
            title: "LINT FIRST".to_owned(),
            body: "Run the linter before anything.".to_owned(),
            category: "rtl".to_owned(),
            evidence: "It caught it on WAY-123.".to_owned(),
        },
        person(),
    )
    .await
    .expect("revised");
    assert_eq!(revised.learning.id, RESTORED);
    assert_eq!(revised.learning.title, "LINT FIRST");

    // Saving a deleted learning's title brings it back.
    let brought_back = learning::save(
        &mut connection,
        LearningContent {
            title: "Old advice".to_owned(),
            body: "Reboot it, then check the logs.".to_owned(),
            category: "pitfall".to_owned(),
            evidence: "It came back.".to_owned(),
        },
        person(),
    )
    .await
    .expect("revived");
    assert_eq!(brought_back.learning.id, DELETED);
    assert_eq!(brought_back.learning.deleted_at, None);
    let kinds: Vec<&str> = brought_back
        .history
        .iter()
        .map(|entry| entry.kind.as_str())
        .collect();
    assert_eq!(kinds, ["restored", "revised"]);

    // An entry from before the move undoes as it did, field by field: what it wrote is put back,
    // and a field changed again since keeps its newer value.
    let undone = learning::undo(&mut connection, REVISED, REVISED_TITLE, person())
        .await
        .expect("undone");
    assert_eq!(
        undone.learning.title,
        "Rebuild indexes after a collation change"
    );
    assert_eq!(undone.history[0].data["undid"], json!(REVISED_TITLE));
    let again = learning::undo(
        &mut connection,
        REVISED,
        REVISED_BODY_AND_CATEGORY,
        person(),
    )
    .await
    .expect("undone");
    assert_eq!(
        again.learning.category, "procedure",
        "the category it wrote is put back"
    );
    assert_eq!(
        again.learning.body, "Run REINDEX.",
        "the body had already changed"
    );
    assert!(matches!(
        learning::undo(&mut connection, REVISED, REVISED_CREATED, person()).await,
        Ok(recorded) if recorded.learning.deleted_at.is_some()
    ));
    assert!(matches!(
        learning::undo(&mut connection, REVISED, REVISED_TITLE, person()).await,
        Err(LearningError::Deleted)
    ));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn what_was_true_of_the_doc_still_holds_on_moved_data() {
    let (url, mut connection) = laid_out_as_production().await;
    up(&url).await;

    // A section's version from before the move restores as any page's does.
    let glossary = Section::Glossary.page_id();
    page::restore_version(&mut connection, glossary, 1, person())
        .await
        .expect("restored");
    let sections = company_doc::sections(&mut connection).await.expect("reads");
    let glossary_now = sections
        .iter()
        .find(|section| section.section == Section::Glossary)
        .expect("every section");
    assert_eq!(glossary_now.content, "");

    // A proposal made before the move is a suggestion that merges three ways with what changed
    // since: here, against a section emptied since, its lines overlap and it stays pending.
    assert!(matches!(
        page_suggestion::accept(&mut connection, glossary, PENDING, None, person()).await,
        Err(SuggestionError::Overlaps)
    ));

    // The fixed pages keep their place, whoever asks.
    for id in [
        LEARNINGS_PAGE_ID,
        COMPANY_PAGE_ID,
        Section::Glossary.page_id(),
    ] {
        assert!(matches!(
            page::delete(&mut connection, id, person(), None).await,
            Err(PageError::Fixed)
        ));
    }
}
