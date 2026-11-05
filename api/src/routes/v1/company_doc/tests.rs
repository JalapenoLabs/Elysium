// Copyright © 2026 Jalapeno Labs

//! The company doc against Postgres (#72): a person's edits and every change recorded and undone,
//! proposals waiting until a person approves each one, approvals merged with what changed since,
//! the knowledge index following each change in the same transaction, and guests refused.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use std::collections::BTreeMap;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::Utc;
use diesel::QueryableByName;
use diesel::sql_types::{BigInt, Text, Uuid as SqlUuid};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use tower::ServiceExt as _;

use crate::company_doc::{self, ProposeRequest};
use crate::models::agent::Agent;
use crate::models::company_doc::{
    self as doc, Author, CompanyDocError, Edit, HistoryKind, ProposalState, Proposer, Section,
};
use crate::models::user::{CODING_AGENT_USER_ID, ELYSIA_USER_ID};
use crate::realtime::EventBus;
use crate::routes;
use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database, signed_in_cookie};

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

fn author() -> Author {
    Author {
        user_id: TEST_PERSON_ID,
        at: Utc::now(),
    }
}

fn adding(section: Section, line: &str) -> ProposeRequest {
    ProposeRequest {
        section,
        edits: vec![Edit {
            find: String::new(),
            replace: line.to_owned(),
        }],
        reason: "Said in the kickoff.".to_owned(),
        quote: Some(line.to_owned()),
        source: Some("Kickoff".to_owned()),
    }
}

async fn content_of(connection: &mut AsyncPgConnection, section: Section) -> String {
    doc::sections(connection)
        .await
        .expect("read")
        .into_iter()
        .find(|stored| stored.section == section)
        .expect("every section exists")
        .content
}

/// How many indexed passages of `section` match `query`, parsed as searches parse it.
async fn matches(connection: &mut AsyncPgConnection, section: Section, query: &str) -> i64 {
    diesel::sql_query(
        "SELECT count(*) AS count FROM knowledge_passages p \
         JOIN knowledge_documents d ON d.id = p.document_id \
         WHERE d.page_id = $1 AND d.source = 'company_doc' \
           AND p.search_vector @@ websearch_to_tsquery('english', $2)",
    )
    .bind::<SqlUuid, _>(section.page_id())
    .bind::<Text, _>(query)
    .get_result::<Count>(connection)
    .await
    .expect("the search runs")
    .count
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_doc_starts_with_every_section_empty_and_out_of_the_index() {
    let (_url, mut connection) = migrated_database().await;

    let read = company_doc::read(&mut connection).await.expect("read");

    let keys: Vec<Section> = read
        .sections
        .iter()
        .map(|section| section.section)
        .collect();
    assert_eq!(keys, Section::ALL);
    assert!(
        read.sections
            .iter()
            .all(|section| section.content.is_empty())
    );
    let indexed: Count = diesel::sql_query(
        "SELECT count(*) AS count FROM knowledge_documents WHERE source = 'company_doc'",
    )
    .get_result(&mut connection)
    .await
    .expect("counts");
    assert_eq!(indexed.count, 0, "an empty section has nothing to find");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn an_edit_is_recorded_indexed_and_undone_section_by_section() {
    let (_url, mut connection) = migrated_database().await;

    let edited = doc::edit(
        &mut connection,
        BTreeMap::from([
            (Section::People, "- Priya Raman, operations".to_owned()),
            (Section::Glossary, "- WAY-123: the launch ticket".to_owned()),
            (Section::Company, String::new()),
        ]),
        author(),
    )
    .await
    .expect("edited");
    let entry = edited.expect("recorded");
    assert_eq!(entry.kind, HistoryKind::Edited.as_str());
    assert_eq!(
        entry.data["changes"],
        json!({
            "people": { "from": "", "to": "- Priya Raman, operations" },
            "glossary": { "from": "", "to": "- WAY-123: the launch ticket" },
        }),
        "a section given its own text again is not part of the change"
    );
    assert_eq!(
        matches(&mut connection, Section::Glossary, "WAY-123").await,
        1
    );
    assert_eq!(
        matches(&mut connection, Section::People, "Priya in operations").await,
        1
    );

    // Someone changes the glossary again, then the first edit is undone.
    doc::edit(
        &mut connection,
        BTreeMap::from([(Section::Glossary, "- WAY-124: the next one".to_owned())]),
        author(),
    )
    .await
    .expect("edited again");
    let undone = doc::undo(&mut connection, entry.id, author())
        .await
        .expect("undone");

    let undo_entry = undone.expect("recorded");
    assert_eq!(undo_entry.data["undid"], json!(entry.id));
    assert_eq!(content_of(&mut connection, Section::People).await, "");
    assert_eq!(
        content_of(&mut connection, Section::Glossary).await,
        "- WAY-124: the next one",
        "a section changed since keeps its newer text"
    );
    assert_eq!(
        matches(&mut connection, Section::People, "Priya").await,
        0,
        "an emptied section leaves the index"
    );
    assert!(matches!(
        doc::undo(&mut connection, entry.id, author()).await,
        Err(CompanyDocError::NothingToUndo(_))
    ));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_proposal_changes_nothing_until_approved_and_merges_with_what_changed_since() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    doc::edit(
        &mut connection,
        BTreeMap::from([(Section::Glossary, "- Alpha\n- Delta\n- Kilo".to_owned())]),
        author(),
    )
    .await
    .expect("edited");

    let agent = Agent::mcp_client("claude-code-client", "Claude Code").expect("recordable");
    let through_client = Proposer::Person {
        user_id: TEST_PERSON_ID,
        agent: Some(&agent),
    };
    let proposed = company_doc::propose(
        &mut connection,
        &events,
        adding(Section::Glossary, "- Sierra"),
        through_client,
    )
    .await
    .expect("proposed");
    assert!(proposed.created);
    assert_eq!(
        content_of(&mut connection, Section::Glossary).await,
        "- Alpha\n- Delta\n- Kilo",
        "proposing changes nothing"
    );
    let again = company_doc::propose(
        &mut connection,
        &events,
        adding(Section::Glossary, "- Sierra"),
        Proposer::Session(12),
    )
    .await
    .expect("answered");
    assert!(!again.created, "the same change is pending once");
    assert_eq!(again.proposal.id, proposed.proposal.id);

    // A person adds a line near the top before approving.
    doc::edit(
        &mut connection,
        BTreeMap::from([(
            Section::Glossary,
            "- Alpha\n- Bravo\n- Delta\n- Kilo".to_owned(),
        )]),
        author(),
    )
    .await
    .expect("edited");
    let approved = doc::approve(&mut connection, proposed.proposal.id, author())
        .await
        .expect("approved");

    assert_eq!(approved.state, ProposalState::Approved.as_str());
    assert_eq!(approved.decided_by, Some(TEST_PERSON_ID));
    assert_eq!(
        content_of(&mut connection, Section::Glossary).await,
        "- Alpha\n- Bravo\n- Delta\n- Kilo\n- Sierra"
    );
    let history = doc::history(&mut connection).await.expect("read");
    let entry = history.last().expect("recorded");
    assert_eq!(
        (entry.kind.as_str(), entry.proposal_id),
        (HistoryKind::Approved.as_str(), Some(proposed.proposal.id))
    );
    assert!(matches!(
        doc::approve(&mut connection, proposed.proposal.id, author()).await,
        Err(CompanyDocError::AlreadyDecided(_))
    ));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_proposal_whose_lines_changed_since_stays_pending_and_rejecting_one_writes_nothing() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    doc::edit(
        &mut connection,
        BTreeMap::from([(Section::People, "- Priya Ramen, operations".to_owned())]),
        author(),
    )
    .await
    .expect("edited");
    let request = ProposeRequest {
        edits: vec![Edit {
            find: "Ramen".to_owned(),
            replace: "Rahman".to_owned(),
        }],
        ..adding(Section::People, "unused")
    };
    let proposed = company_doc::propose(
        &mut connection,
        &events,
        request,
        Proposer::Elysia {
            workflow_run_id: None,
        },
    )
    .await
    .expect("proposed");
    assert_eq!(proposed.proposal.created_by, ELYSIA_USER_ID);

    doc::edit(
        &mut connection,
        BTreeMap::from([(Section::People, "- Priya Raman, operations".to_owned())]),
        author(),
    )
    .await
    .expect("edited");
    assert!(matches!(
        doc::approve(&mut connection, proposed.proposal.id, author()).await,
        Err(CompanyDocError::Overlaps(Section::People))
    ));
    let rejected = doc::reject(&mut connection, proposed.proposal.id, author())
        .await
        .expect("rejected");
    assert_eq!(rejected.state, ProposalState::Rejected.as_str());
    assert_eq!(
        content_of(&mut connection, Section::People).await,
        "- Priya Raman, operations"
    );
    assert_eq!(
        doc::history(&mut connection).await.expect("read").len(),
        2,
        "a rejection is not a change to the doc"
    );

    let session = company_doc::propose(
        &mut connection,
        &events,
        adding(Section::Products, "- Atlas"),
        Proposer::Session(7),
    )
    .await
    .expect("proposed");
    assert_eq!(
        (
            session.proposal.created_by,
            session
                .proposal
                .agent
                .as_ref()
                .map(|agent| agent.id.as_str())
        ),
        (CODING_AGENT_USER_ID, Some("session:7"))
    );
}

/// A router test's answer to `method path` with `body`, as the test person.
async fn call(
    state: &crate::state::AppState,
    method: Method,
    path: &str,
    body: Option<&Value>,
) -> (StatusCode, Value) {
    let cookie = signed_in_cookie(state).await;
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::COOKIE, cookie)
        .header(header::ORIGIN, "http://localhost:4000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
        .expect("a request builds");
    let response = routes::router(state)
        .with_state(state.clone())
        .oneshot(request)
        .await
        .expect("the router answers");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("the body reads");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn members_edit_and_decide_through_the_routes_and_guests_reach_nothing() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;

    let (status, edited) = call(
        &state,
        Method::PATCH,
        "/api/v1/company-doc",
        Some(&json!({ "sections": { "people": "  - Priya Raman  " } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(edited["sections"][2]["section"], "people");
    assert_eq!(edited["sections"][2]["content"], "- Priya Raman", "trimmed");

    let proposed = company_doc::propose(
        &mut connection,
        &EventBus::in_process(),
        adding(Section::People, "- Tom Okafor"),
        Proposer::Session(3),
    )
    .await
    .expect("proposed");
    let (_, read) = call(&state, Method::GET, "/api/v1/company-doc", None).await;
    assert_eq!(
        read["sections"][2]["proposals"][0]["id"],
        json!(proposed.proposal.id),
        "a pending proposal is answered beside its section"
    );

    let approve = format!(
        "/api/v1/company-doc/proposals/{}/approve",
        proposed.proposal.id
    );
    let (status, approved) = call(&state, Method::POST, &approve, None).await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    assert_eq!(approved["proposal"]["state"], "approved");
    assert_eq!(
        approved["doc"]["sections"][2]["content"],
        "- Priya Raman\n- Tom Okafor"
    );
    let (status, _) = call(&state, Method::POST, &approve, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "a decision is made once");

    let (_, history) = call(&state, Method::GET, "/api/v1/company-doc/history", None).await;
    let entries = history["history"].as_array().expect("a list");
    assert_eq!(entries.len(), 2);
    let undo = format!(
        "/api/v1/company-doc/history/{}/undo",
        entries[1]["id"].as_str().unwrap()
    );
    let (status, undone) = call(&state, Method::POST, &undo, None).await;
    assert_eq!(status, StatusCode::OK, "{undone}");
    assert_eq!(undone["sections"][2]["content"], "- Priya Raman");

    diesel::sql_query(format!(
        "UPDATE users SET role = 'guest' WHERE id = '{TEST_PERSON_ID}'"
    ))
    .execute(&mut connection)
    .await
    .expect("the role changes");
    for (method, path) in [
        (Method::GET, "/api/v1/company-doc".to_owned()),
        (Method::PATCH, "/api/v1/company-doc".to_owned()),
        (Method::GET, "/api/v1/company-doc/history".to_owned()),
        (Method::POST, undo.clone()),
        (Method::POST, approve.clone()),
        (Method::POST, "/api/v1/company-doc/review".to_owned()),
    ] {
        let body = json!({ "sections": { "people": "x" } });
        let (status, refused) = call(&state, method.clone(), &path, Some(&body)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
        assert_eq!(refused["code"], "members_only", "{method} {path}");
    }
    assert_eq!(
        content_of(&mut connection, Section::People).await,
        "- Priya Raman",
        "nothing a guest sent was written"
    );
}
