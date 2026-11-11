// Copyright © 2026 Jalapeno Labs

//! The migration that makes suggestions every page's (#262), against the company doc's proposals as
//! production holds them before: pending, rejected, and approved ones, made through an MCP client, a
//! session, a Project Manager, and by Elysia, one made against text a version published a moment
//! after it was stamped, and one made against text no version holds.
//!
//! Up must carry every proposal to its section's page under its own id, naming the version it was
//! made from, and an applied one's version must name the suggestion it now is. Down must hand every
//! row back exactly, and up must apply again.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs it.

use chrono::Utc;
use diesel::QueryableByName;
use diesel::sql_types::Jsonb;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl, SimpleAsyncConnection};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use uuid::{Uuid, uuid};

use super::migrations::{self, MIGRATIONS, MigrationCommand};
use crate::models::agent::Author;
use crate::models::company_doc::{self, Section};
use crate::models::page::{self, PageEdit};
use crate::models::page_suggestion::{self, Edit, NewSuggestion, SuggestionState, review_item};
use crate::models::user::{CODING_AGENT_USER_ID, ELYSIA_USER_ID, PROJECT_MANAGER_USER_ID};
use crate::test_support::{TEST_PERSON_ID, empty_database};

/// The migration under test.
const SUGGEST: &str = "2026-11-10-060000-0262_suggest_edits_to_pages";

const SECOND_PERSON: Uuid = uuid!("0199a3c4-0000-7000-8000-00000000b0b0");

/// Through an MCP client, still waiting.
const PENDING: Uuid = uuid!("0199a3c6-0000-7000-8000-000000000c01");
/// A session's, turned down.
const REJECTED: Uuid = uuid!("0199a3c6-0000-7000-8000-000000000c02");
/// Elysia's, applied by the second person.
const APPROVED: Uuid = uuid!("0199a3c6-0000-7000-8000-000000000c03");
/// A Project Manager's, stamped a moment before the version holding its text was published.
const EARLY: Uuid = uuid!("0199a3c6-0000-7000-8000-000000000c04");
/// Made against text no version holds.
const UNMATCHED: Uuid = uuid!("0199a3c6-0000-7000-8000-000000000c05");

/// The entry that published the approved proposal.
const APPLIED_ENTRY: Uuid = uuid!("0199a3c6-0000-7000-8000-000000000b03");

/// What production holds before suggestions were every page's.
const BEFORE: &str = r#"
INSERT INTO users (id, kratos_identity_id, email, name, role, status, approved_at) VALUES
 ('0199a3c4-0000-7000-8000-00000000beef', '0199a3c4-0000-7000-8000-00000000cafe',
  'tester@example.com', 'Tester', 'admin', 'active', now()),
 ('0199a3c4-0000-7000-8000-00000000b0b0', '0199a3c4-0000-7000-8000-00000000d00d',
  'second@example.com', 'Second', 'member', 'active', now());

INSERT INTO page_versions (page_id, version, title, body, created_by, created_at) VALUES
 ('00000000-0000-0000-0002-000000000003', 2, 'People', '- Priya',
  '0199a3c4-0000-7000-8000-00000000beef', '2026-10-05 10:00:00+00'),
 ('00000000-0000-0000-0002-000000000003', 3, 'People', '- Priya Raman',
  '0199a3c4-0000-7000-8000-00000000b0b0', '2026-10-05 12:00:00+00'),
 ('00000000-0000-0000-0002-000000000007', 2, 'Glossary', '- ACME: a client',
  '0199a3c4-0000-7000-8000-00000000beef', '2026-10-05 10:00:00+00');
INSERT INTO page_events (id, page_id, kind, created_by, data, created_at) VALUES
 ('0199a3c6-0000-7000-8000-000000000b02', '00000000-0000-0000-0002-000000000003', 'published',
  '0199a3c4-0000-7000-8000-00000000beef', '{"version": 2}', '2026-10-05 10:00:00+00'),
 ('0199a3c6-0000-7000-8000-000000000b03', '00000000-0000-0000-0002-000000000003', 'published',
  '0199a3c4-0000-7000-8000-00000000b0b0',
  '{"version": 3, "proposalId": "0199a3c6-0000-7000-8000-000000000c03"}',
  '2026-10-05 12:00:00+00'),
 ('0199a3c6-0000-7000-8000-000000000b02', '00000000-0000-0000-0002-000000000007', 'published',
  '0199a3c4-0000-7000-8000-00000000beef', '{"version": 2}', '2026-10-05 10:00:00+00');
ALTER TABLE pages DISABLE TRIGGER set_updated_at;
UPDATE pages SET body = '- Priya Raman', current_version = 3, updated_at = '2026-10-05 12:00:00+00'
WHERE id = '00000000-0000-0000-0002-000000000003';
UPDATE pages SET body = '- ACME: a client', current_version = 2,
                 updated_at = '2026-10-05 10:00:00+00'
WHERE id = '00000000-0000-0000-0002-000000000007';
ALTER TABLE pages ENABLE TRIGGER set_updated_at;

INSERT INTO company_doc_proposals (id, section, base_content, content, reason, quote, source,
                                   state, decided_by, decided_at, created_by, agent, agent_name,
                                   created_at) VALUES
 ('0199a3c6-0000-7000-8000-000000000c01', 'glossary', '- ACME: a client',
  E'- ACME: a client\n- WAY: the tracker', 'Said in the kickoff.', 'We call it WAY.',
  'Kickoff, October 1st', 'pending', NULL, NULL,
  '0199a3c4-0000-7000-8000-00000000beef', 'mcp:client-one', 'Claude Code',
  '2026-10-06 09:00:00+00'),
 ('0199a3c6-0000-7000-8000-000000000c02', 'people', '- Priya', '- Pria', 'A typo.', NULL, NULL,
  'rejected', '0199a3c4-0000-7000-8000-00000000beef', '2026-10-05 11:00:00+00',
  '00000000-0000-0000-0000-000000000003', 'session:12', NULL, '2026-10-05 10:30:00+00'),
 ('0199a3c6-0000-7000-8000-000000000c03', 'people', '- Priya', '- Priya Raman', 'Her full name.',
  NULL, NULL, 'approved', '0199a3c4-0000-7000-8000-00000000b0b0', '2026-10-05 12:00:00+00',
  '00000000-0000-0000-0000-000000000002', NULL, NULL, '2026-10-05 11:00:00+00'),
 ('0199a3c6-0000-7000-8000-000000000c04', 'glossary', '- ACME: a client',
  E'- ACME: a client\n- Beta: a product', 'On the roadmap.', NULL, NULL, 'pending', NULL, NULL,
  '00000000-0000-0000-0000-000000000004',
  'project_manager:0199a3c6-0000-7000-8000-0000000000aa', NULL, '2026-10-05 09:59:59+00'),
 ('0199a3c6-0000-7000-8000-000000000c05', 'people', '- Somebody else', '- Somebody', 'Shorter.',
  NULL, NULL, 'pending', NULL, NULL,
  '0199a3c4-0000-7000-8000-00000000beef', NULL, NULL, '2026-10-05 11:30:00+00');
"#;

#[derive(QueryableByName)]
struct Rows {
    #[diesel(sql_type = Jsonb)]
    rows: Value,
}

/// How many migrations come after [`SUGGEST`], plus [`SUGGEST`] itself, so reverting that many
/// lands just before it however many land after it.
fn from_suggest() -> u32 {
    let names: Vec<String> =
        diesel::migration::MigrationSource::<diesel::pg::Pg>::migrations(&MIGRATIONS)
            .expect("embedded migrations load")
            .iter()
            .map(|migration| migration.name().to_string())
            .collect();
    let position = names
        .iter()
        .position(|name| name == SUGGEST)
        .expect("the migration is embedded");
    u32::try_from(names.len() - position).expect("a handful of migrations")
}

async fn up(url: &SecretString) {
    migrations::execute(url.clone(), MigrationCommand::Run)
        .await
        .expect("the migration applies");
}

async fn down(url: &SecretString) {
    let revert = MigrationCommand::Revert {
        count: from_suggest(),
    };
    migrations::execute(url.clone(), revert)
        .await
        .expect("the migration rolls back");
}

/// A database holding [`BEFORE`], just before the migration, and a connection to it.
async fn laid_out_as_production() -> (SecretString, AsyncPgConnection) {
    let url = empty_database().await;
    up(&url).await;
    down(&url).await;
    let mut connection = AsyncPgConnection::establish(url.expose_secret())
        .await
        .expect("connects");
    connection
        .batch_execute(BEFORE)
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

/// What down must hand back, by table.
async fn as_they_were(connection: &mut AsyncPgConnection) -> Vec<Value> {
    vec![
        rows_of(connection, "SELECT * FROM company_doc_proposals").await,
        rows_of(connection, "SELECT * FROM page_events").await,
        rows_of(connection, "SELECT * FROM page_versions").await,
    ]
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn every_proposal_becomes_a_suggestion_to_its_sections_page_naming_its_version() {
    let (url, mut connection) = laid_out_as_production().await;
    up(&url).await;

    let moved = rows_of(
        &mut connection,
        "SELECT id, page_id, base_version, base_body, body, state, dismissal_reason, \
                accepted_changes, decided_by, created_by, agent, agent_name \
         FROM page_suggestions",
    )
    .await;
    let by_id = |id: Uuid| -> Value {
        moved
            .as_array()
            .expect("rows")
            .iter()
            .find(|row| row["id"] == json!(id))
            .unwrap_or_else(|| panic!("{id} was moved"))
            .clone()
    };
    let people = Section::People.page_id();
    let glossary = Section::Glossary.page_id();

    let pending = by_id(PENDING);
    assert_eq!(pending["page_id"], json!(glossary));
    assert_eq!(pending["base_version"], 2);
    assert_eq!(pending["state"], "pending");
    assert_eq!(
        (&pending["agent"], &pending["agent_name"]),
        (&json!("mcp:client-one"), &json!("Claude Code"))
    );
    let rejected = by_id(REJECTED);
    assert_eq!(
        (
            &rejected["page_id"],
            &rejected["state"],
            &rejected["base_version"]
        ),
        (&json!(people), &json!("dismissed"), &json!(2))
    );
    assert_eq!(
        rejected["dismissal_reason"],
        Value::Null,
        "no reason was asked then"
    );
    assert_eq!(
        (&rejected["created_by"], &rejected["agent"]),
        (&json!(CODING_AGENT_USER_ID), &json!("session:12")),
        "who it was by stays as recorded"
    );
    let approved = by_id(APPROVED);
    assert_eq!(approved["state"], "accepted");
    assert_eq!(approved["accepted_changes"], Value::Null, "every change");
    assert_eq!(approved["created_by"], json!(ELYSIA_USER_ID));
    assert_eq!(approved["decided_by"], json!(SECOND_PERSON));
    let early = by_id(EARLY);
    assert_eq!(
        early["base_version"], 2,
        "the version holding its text, though published a moment after it"
    );
    assert_eq!(early["created_by"], json!(PROJECT_MANAGER_USER_ID));
    let unmatched = by_id(UNMATCHED);
    assert_eq!(
        (&unmatched["base_version"], &unmatched["base_body"]),
        (&json!(2), &json!("- Somebody else")),
        "the version published by then, with the text it was made against kept"
    );

    let applied = rows_of(
        &mut connection,
        &format!("SELECT data FROM page_events WHERE id = '{APPLIED_ENTRY}'"),
    )
    .await;
    assert_eq!(
        applied,
        json!([{ "data": { "version": 3, "suggestionId": APPROVED } }]),
        "the version names the suggestion it applied"
    );

    let waiting = company_doc::pending(&mut connection).await.expect("reads");
    let ids: Vec<Uuid> = waiting
        .iter()
        .map(|suggested| suggested.suggestion.id)
        .collect();
    assert_eq!(
        ids,
        [PENDING, EARLY, UNMATCHED],
        "in the order of their ids, which the API makes in the order it suggests"
    );
    let counts = page_suggestion::pending_counts(&mut connection, None)
        .await
        .expect("counts");
    assert_eq!(
        (counts.get(&glossary), counts.get(&people)),
        (Some(&2), Some(&1))
    );
    let dismissed = company_doc::latest(&mut connection, SuggestionState::Dismissed, 10)
        .await
        .expect("reads");
    assert_eq!(dismissed[0].section, Section::People);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_pages_carried_over_with_suggestions_waiting_are_told_once_as_the_engine_starts() {
    let (url, mut connection) = laid_out_as_production().await;
    up(&url).await;

    let raised = review_item::reconcile(&mut connection, Utc::now())
        .await
        .expect("reconciled");
    let mut told: Vec<String> = raised
        .iter()
        .map(|item| item.record.title.clone())
        .collect();
    told.sort();
    assert_eq!(
        told,
        [
            "Review 2 suggested edits to Glossary",
            "Review a suggested edit to People",
        ]
    );
    let again = review_item::reconcile(&mut connection, Utc::now())
        .await
        .expect("reconciled");
    assert!(again.is_empty(), "a page told is not told twice");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn down_hands_back_every_proposal_exactly_and_up_applies_again() {
    let (url, mut connection) = laid_out_as_production().await;
    let before = as_they_were(&mut connection).await;

    up(&url).await;
    let once = rows_of(&mut connection, "SELECT * FROM page_suggestions").await;
    down(&url).await;
    let after = as_they_were(&mut connection).await;
    for (index, (then, now)) in before.iter().zip(&after).enumerate() {
        assert_eq!(now, then, "table {index} is not what it was");
    }

    up(&url).await;
    let twice = rows_of(&mut connection, "SELECT * FROM page_suggestions").await;
    assert_eq!(
        twice, once,
        "the migration lands the same way the second time"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn down_keeps_the_doc_and_lets_go_of_what_only_other_pages_could_hold() {
    let (url, mut connection) = laid_out_as_production().await;
    up(&url).await;

    // Since: a suggestion to an ordinary page, accepted, and one to the glossary, accepted.
    let person = Author {
        user_id: TEST_PERSON_ID,
        agent: None,
        at: Utc::now(),
    };
    let edit = PageEdit {
        title: "Runbook".to_owned(),
        body: "Restart the worker.".to_owned(),
        learning: None,
    };
    let runbook = page::create(&mut connection, None, edit, person)
        .await
        .expect("created")
        .page
        .id;
    let add = |line: &str| {
        vec![Edit {
            find: String::new(),
            replace: line.to_owned(),
        }]
    };
    let to_runbook = NewSuggestion::new(runbook, None, add("Then the queue."), "Why", None, None)
        .expect("in bounds");
    let on_runbook = page_suggestion::suggest(&mut connection, to_runbook, person)
        .await
        .expect("suggested");
    page_suggestion::accept(
        &mut connection,
        runbook,
        on_runbook.suggestion.id,
        None,
        person,
    )
    .await
    .expect("accepted");
    page_suggestion::accept(
        &mut connection,
        Section::Glossary.page_id(),
        PENDING,
        None,
        person,
    )
    .await
    .expect("accepted");

    down(&url).await;

    let proposals = rows_of(
        &mut connection,
        "SELECT id, state, decided_by FROM company_doc_proposals",
    )
    .await;
    let proposals = proposals.as_array().expect("rows");
    assert_eq!(proposals.len(), 5, "only the doc's");
    let pending = proposals
        .iter()
        .find(|row| row["id"] == json!(PENDING))
        .expect("kept");
    assert_eq!(
        (&pending["state"], &pending["decided_by"]),
        (&json!("approved"), &json!(TEST_PERSON_ID))
    );
    let named = rows_of(
        &mut connection,
        "SELECT page_id, data FROM page_events WHERE data ? 'proposalId' OR data ? 'suggestionId'",
    )
    .await;
    let named = named.as_array().expect("rows");
    assert_eq!(named.len(), 2, "{named:?}");
    for row in named {
        assert!(
            Section::of_page(serde_json::from_value(row["page_id"].clone()).expect("an id"))
                .is_some()
        );
        assert!(row["data"].get("proposalId").is_some(), "{row}");
    }

    up(&url).await;
    let glossary = page::find(&mut connection, Section::Glossary.page_id())
        .await
        .expect("seeded");
    assert_eq!(glossary.body, "- ACME: a client\n- WAY: the tracker");
}
