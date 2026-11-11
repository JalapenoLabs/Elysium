// Copyright © 2026 Jalapeno Labs

//! Suggested edits against Postgres (#262): a suggestion changing nothing until a person accepts
//! it, the same edit pending once, accepting wholly or in part merged with what changed since, a
//! change whose lines changed since refused, dismissing with a reason, the one inbox item per page,
//! the company doc's sections taking suggestions through the same framework, and the routes
//! answering members alone.
//!
//! Those are `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs
//! them.

use axum::http::{Method, StatusCode};
use chrono::Utc;
use diesel::QueryableByName;
use diesel::sql_types::{Text, Uuid as SqlUuid};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{ELYSIA_USER_ID, author, call, create, edit, set_role};
use crate::action_items::{Actor, Transition};
use crate::company_doc::{self, ProposeRequest};
use crate::models::action_item::{self, ActionItemState};
use crate::models::agent::{Agent, Author};
use crate::models::company_doc::{self as doc, Section};
use crate::models::learning::{self, LearningContent};
use crate::models::page::{self, PageError};
use crate::models::page_suggestion::{
    self, Edit, NewSuggestion, Suggested, SuggestionError, SuggestionState,
};
use crate::realtime::EventBus;
use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database};

fn adding(line: &str) -> Vec<Edit> {
    vec![Edit {
        find: String::new(),
        replace: line.to_owned(),
    }]
}

fn replacing(find: &str, replace: &str) -> Vec<Edit> {
    vec![Edit {
        find: find.to_owned(),
        replace: replace.to_owned(),
    }]
}

fn elysia() -> Author<'static> {
    Author {
        user_id: ELYSIA_USER_ID,
        agent: None,
        at: Utc::now(),
    }
}

/// Publishes `body` over page `id` as the test person, from whatever version it is at.
async fn publish(connection: &mut AsyncPgConnection, id: Uuid, body: &str) {
    let current = page::find(connection, id).await.expect("found");
    page::publish(
        connection,
        id,
        edit(&current.title, body),
        current.current_version,
        author(None),
    )
    .await
    .expect("published");
}

async fn suggest(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    base_version: Option<i32>,
    edits: Vec<Edit>,
    by: Author<'_>,
) -> Result<Suggested, SuggestionError> {
    let new = NewSuggestion::new(id, base_version, edits, "Heard in the kickoff.", None, None)
        .expect("in bounds");
    page_suggestion::suggest(connection, new, by).await
}

async fn body_of(connection: &mut AsyncPgConnection, id: Uuid) -> String {
    page::find(connection, id).await.expect("found").body
}

#[derive(QueryableByName)]
struct ItemRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = Text)]
    title: String,
}

/// The inbox item page `id` has, if it has one, by its row in `page_suggestion_items`.
async fn review_item_of(connection: &mut AsyncPgConnection, id: Uuid) -> Option<ItemRow> {
    diesel::sql_query(
        "SELECT items.id, items.title FROM page_suggestion_items AS suggestion_items \
         JOIN action_items AS items ON items.id = suggestion_items.action_item_id \
         WHERE suggestion_items.page_id = $1",
    )
    .bind::<SqlUuid, _>(id)
    .get_result(connection)
    .await
    .ok()
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_suggestion_changes_nothing_until_accepted_and_merges_with_what_changed_since() {
    let (_url, mut connection) = migrated_database().await;
    let id = create(&mut connection, None, "Runbook").await;
    publish(&mut connection, id, "- Alpha\n- Delta\n- Kilo").await;

    let agent = Agent::mcp_client("claude-code-client", "Claude Code").expect("recordable");
    let suggested = suggest(
        &mut connection,
        id,
        Some(2),
        adding("- Sierra"),
        author(Some(&agent)),
    )
    .await
    .expect("suggested");
    assert!(suggested.created);
    let suggestion = &suggested.suggestion;
    assert_eq!(suggestion.base_version, 2);
    assert_eq!(
        (suggestion.created_by, suggestion.agent.as_deref()),
        (TEST_PERSON_ID, Some("mcp:claude-code-client"))
    );
    assert_eq!(
        body_of(&mut connection, id).await,
        "- Alpha\n- Delta\n- Kilo",
        "suggesting changes nothing"
    );
    let again = suggest(&mut connection, id, None, adding("- Sierra"), elysia())
        .await
        .expect("answered");
    assert!(!again.created, "the same edit is pending once");
    assert_eq!(again.suggestion.id, suggestion.id);

    // A person adds a line near the top before accepting.
    publish(&mut connection, id, "- Alpha\n- Bravo\n- Delta\n- Kilo").await;
    let decided = page_suggestion::accept(&mut connection, id, suggestion.id, None, author(None))
        .await
        .expect("accepted");

    assert_eq!(decided.suggestion.state, SuggestionState::Accepted.as_str());
    assert_eq!(decided.suggestion.decided_by, Some(TEST_PERSON_ID));
    assert_eq!(decided.suggestion.accepted_changes, None, "every change");
    assert_eq!(
        body_of(&mut connection, id).await,
        "- Alpha\n- Bravo\n- Delta\n- Kilo\n- Sierra"
    );
    let published = decided.published.expect("a version");
    let entry = published.history.last().expect("recorded");
    assert_eq!(entry.data["suggestionId"], json!(suggestion.id));
    assert_eq!(entry.created_by, TEST_PERSON_ID, "accepted by the person");
    assert!(matches!(
        page_suggestion::accept(&mut connection, id, suggestion.id, None, author(None)).await,
        Err(SuggestionError::AlreadyDecided("accepted"))
    ));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn accepting_in_part_takes_the_changes_chosen_and_leaves_the_rest_as_they_were() {
    let (_url, mut connection) = migrated_database().await;
    let id = create(&mut connection, None, "People").await;
    publish(
        &mut connection,
        id,
        "- Priya Ramen\n- Tom\n- Uma\n- Vik\n- Wes",
    )
    .await;
    let edits = vec![
        Edit {
            find: "Ramen".to_owned(),
            replace: "Raman".to_owned(),
        },
        Edit {
            find: String::new(),
            replace: "- Xia".to_owned(),
        },
    ];
    let suggested = suggest(&mut connection, id, None, edits, elysia())
        .await
        .expect("suggested");
    let changes =
        page_suggestion::changes(&suggested.suggestion.base_body, &suggested.suggestion.body);
    assert_eq!(changes.len(), 2, "{changes:?}");

    let decided = page_suggestion::accept(
        &mut connection,
        id,
        suggested.suggestion.id,
        Some(vec![1]),
        author(None),
    )
    .await
    .expect("accepted in part");

    assert_eq!(decided.suggestion.accepted_changes, Some(vec![Some(1)]));
    assert_eq!(
        body_of(&mut connection, id).await,
        "- Priya Ramen\n- Tom\n- Uma\n- Vik\n- Wes\n- Xia",
        "the name was left as it was"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_change_whose_lines_changed_since_stays_pending_and_dismissing_one_writes_nothing() {
    let (_url, mut connection) = migrated_database().await;
    let id = create(&mut connection, None, "People").await;
    publish(&mut connection, id, "- Priya Ramen, operations").await;
    let suggested = suggest(
        &mut connection,
        id,
        None,
        replacing("Ramen", "Rahman"),
        elysia(),
    )
    .await
    .expect("suggested");
    assert_eq!(suggested.suggestion.created_by, ELYSIA_USER_ID);
    assert_eq!(suggested.suggestion.agent, None, "Elysia names no agent");

    publish(&mut connection, id, "- Priya Raman, operations").await;
    let versions = page::versions(&mut connection, id)
        .await
        .expect("read")
        .len();
    assert!(matches!(
        page_suggestion::accept(
            &mut connection,
            id,
            suggested.suggestion.id,
            None,
            author(None)
        )
        .await,
        Err(SuggestionError::Overlaps)
    ));
    assert!(matches!(
        page_suggestion::dismiss(
            &mut connection,
            id,
            suggested.suggestion.id,
            "  ",
            author(None)
        )
        .await,
        Err(SuggestionError::Invalid(_))
    ));
    let dismissed = page_suggestion::dismiss(
        &mut connection,
        id,
        suggested.suggestion.id,
        " She spells it Raman ",
        author(None),
    )
    .await
    .expect("dismissed");
    assert_eq!(
        (
            dismissed.suggestion.state.as_str(),
            dismissed.suggestion.dismissal_reason.as_deref()
        ),
        ("dismissed", Some("She spells it Raman"))
    );
    assert_eq!(
        body_of(&mut connection, id).await,
        "- Priya Raman, operations"
    );
    assert_eq!(
        page::versions(&mut connection, id)
            .await
            .expect("read")
            .len(),
        versions,
        "a dismissal is no version"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_suggestion_is_written_against_the_version_it_names_and_refused_where_it_cannot_go() {
    let (_url, mut connection) = migrated_database().await;
    let id = create(&mut connection, None, "Runbook").await;
    publish(&mut connection, id, "Restart the worker.").await;
    publish(&mut connection, id, "Restart the worker, then the queue.").await;

    let older = suggest(
        &mut connection,
        id,
        Some(2),
        replacing("Restart the worker.", "Restart the worker first."),
        elysia(),
    )
    .await
    .expect("suggested against what it read");
    assert_eq!(older.suggestion.base_body, "Restart the worker.");
    assert!(matches!(
        suggest(&mut connection, id, Some(9), adding("x"), elysia()).await,
        Err(SuggestionError::UnknownVersion(9))
    ));
    assert!(matches!(
        suggest(
            &mut connection,
            id,
            None,
            replacing("Nowhere", "x"),
            elysia()
        )
        .await,
        Err(SuggestionError::EditDoesNotApply(_))
    ));
    assert!(matches!(
        suggest(
            &mut connection,
            id,
            Some(2),
            replacing("worker.", "worker, then the queue."),
            elysia()
        )
        .await,
        Err(SuggestionError::ChangesNothing)
    ));

    let learning = learning::save(
        &mut connection,
        LearningContent {
            title: "Lint first".to_owned(),
            body: "Run the linter.".to_owned(),
            category: "convention".to_owned(),
            evidence: "It caught WAY-12.".to_owned(),
        },
        author(None),
    )
    .await
    .expect("saved")
    .learning
    .id;
    assert!(matches!(
        suggest(&mut connection, learning, None, adding("x"), elysia()).await,
        Err(SuggestionError::IsALearning)
    ));
    page::delete(&mut connection, id, author(None), None)
        .await
        .expect("deleted");
    assert!(matches!(
        suggest(&mut connection, id, None, adding("- More"), elysia()).await,
        Err(SuggestionError::Page(PageError::Deleted))
    ));
    let too_long = "x".repeat(doc::CONTENT_MAX_CHARACTERS + 1);
    assert!(matches!(
        suggest(
            &mut connection,
            Section::Glossary.page_id(),
            None,
            adding(&too_long),
            elysia()
        )
        .await,
        Err(SuggestionError::TooLong { max }) if max == doc::CONTENT_MAX_CHARACTERS
    ));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn people_are_told_once_per_page_until_nothing_waits() {
    let (_url, mut connection) = migrated_database().await;
    let id = create(&mut connection, None, "Glossary of terms").await;

    let first = suggest(&mut connection, id, None, adding("- ACME"), elysia())
        .await
        .expect("suggested");
    assert!(first.item.is_some(), "the first raises the page's item");
    let second = suggest(&mut connection, id, None, adding("- Beta"), elysia())
        .await
        .expect("suggested");
    let item = review_item_of(&mut connection, id).await.expect("an item");
    assert_eq!(item.title, "Review 2 suggested edits to Glossary of terms");
    let stored = action_item::find(&mut connection, item.id)
        .await
        .expect("found");
    assert_eq!(stored.state, ActionItemState::Inbox);
    assert_eq!(stored.created_by, crate::models::user::SYSTEM_USER_ID);

    page_suggestion::dismiss(
        &mut connection,
        id,
        first.suggestion.id,
        "Not ours",
        author(None),
    )
    .await
    .expect("dismissed");
    let item = review_item_of(&mut connection, id).await.expect("an item");
    assert_eq!(item.title, "Review a suggested edit to Glossary of terms");
    page_suggestion::accept(
        &mut connection,
        id,
        second.suggestion.id,
        None,
        author(None),
    )
    .await
    .expect("accepted");
    let resolved = action_item::find(&mut connection, item.id)
        .await
        .expect("found");
    assert_eq!(resolved.state, ActionItemState::Resolved, "nothing waits");

    // A person closing a new item by hand is done with it; the next suggestion raises another.
    let third = suggest(&mut connection, id, None, adding("- Gamma"), elysia())
        .await
        .expect("suggested");
    let raised = review_item_of(&mut connection, id).await.expect("an item");
    assert_ne!(raised.id, item.id, "a resolved item is not reopened");
    action_item::transition(
        &mut connection,
        raised.id,
        Transition::Dismiss,
        Actor::User(TEST_PERSON_ID),
        Utc::now(),
    )
    .await
    .expect("dismissed by hand");
    page_suggestion::dismiss(
        &mut connection,
        id,
        third.suggestion.id,
        "Later",
        author(None),
    )
    .await
    .expect("dismissed");
    let after = action_item::find(&mut connection, raised.id)
        .await
        .expect("found");
    assert_eq!(
        after.state,
        ActionItemState::Dismissed,
        "left as its person left it"
    );
    let fourth = suggest(&mut connection, id, None, adding("- Delta"), elysia())
        .await
        .expect("suggested");
    assert!(fourth.item.is_some());
    assert_ne!(
        review_item_of(&mut connection, id)
            .await
            .expect("an item")
            .id,
        raised.id
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_company_doc_takes_suggestions_through_the_same_framework() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let request = ProposeRequest {
        section: Section::People,
        edits: adding("- Priya Raman, operations"),
        reason: "She signs her mail this way.".to_owned(),
        quote: Some("Priya Raman".to_owned()),
        source: Some("Mail, November 3rd".to_owned()),
    };
    let session = Agent::session(7);
    let by_session = Author {
        user_id: TEST_PERSON_ID,
        agent: Some(&session),
        at: Utc::now(),
    };

    let proposed = company_doc::propose(&mut connection, &events, request, by_session)
        .await
        .expect("suggested");

    assert!(proposed.created);
    assert_eq!(proposed.page_id, Section::People.page_id());
    assert_eq!(
        (
            proposed.proposal.created_by,
            proposed
                .proposal
                .agent
                .as_ref()
                .map(|agent| agent.id.as_str())
        ),
        (TEST_PERSON_ID, Some("session:7")),
        "a session suggests as whoever started it, naming the session"
    );
    let read = company_doc::read(&mut connection).await.expect("read");
    let people = read
        .sections
        .iter()
        .find(|section| section.section == Section::People)
        .expect("people");
    assert_eq!(people.content, "", "nothing changes until a person accepts");
    assert_eq!(people.proposals.len(), 1);
    assert_eq!(people.proposals[0].content, "- Priya Raman, operations");
    assert_eq!(people.proposals[0].state, "pending");

    // A person accepts it on the section's page, as they would any page's.
    page_suggestion::accept(
        &mut connection,
        Section::People.page_id(),
        proposed.proposal.id,
        None,
        author(None),
    )
    .await
    .expect("accepted");
    assert_eq!(
        body_of(&mut connection, Section::People.page_id()).await,
        "- Priya Raman, operations"
    );
    let waiting = doc::pending(&mut connection).await.expect("read");
    assert!(waiting.is_empty());
}

/// A runbook with two suggestions waiting: one of two changes, and one of one.
async fn runbook_with_two_suggestions(
    connection: &mut AsyncPgConnection,
) -> (Uuid, Suggested, Suggested) {
    let id = create(connection, None, "Runbook").await;
    publish(connection, id, "- Stop the service\n- Rebuild\n- Start it").await;
    let edits = vec![
        Edit {
            find: "- Stop the service".to_owned(),
            replace: "- Stop the service and its queue".to_owned(),
        },
        Edit {
            find: String::new(),
            replace: "- Check the logs".to_owned(),
        },
    ];
    let suggested = suggest(connection, id, None, edits, elysia())
        .await
        .expect("suggested");
    let other = suggest(connection, id, None, adding("- Tell the team"), elysia())
        .await
        .expect("suggested");
    (id, suggested, other)
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn members_review_suggestions_over_the_routes() {
    let (url, mut connection) = migrated_database().await;
    let mut state = app_state(&url).await;
    state.events = EventBus::in_process();
    let mut heard = state.events.subscribe();
    let (id, suggested, other) = runbook_with_two_suggestions(&mut connection).await;

    let (status, listed) = call(
        &state,
        Method::GET,
        &format!("/api/v1/pages/{id}/suggestions"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let first = &listed["suggestions"][0];
    assert_eq!(first["id"], json!(suggested.suggestion.id));
    assert_eq!(first["changes"].as_array().expect("changes").len(), 2);
    assert_eq!(
        first["changes"][1]["after"],
        "- Rebuild\n- Start it\n- Check the logs"
    );
    assert_eq!(first["changes"][0]["applies"], true);

    let accept = format!(
        "/api/v1/pages/{id}/suggestions/{}/accept",
        suggested.suggestion.id
    );
    let (status, unknown) = call(
        &state,
        Method::POST,
        &accept,
        Some(&json!({ "changes": [5] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{unknown}");
    let (status, accepted) = call(
        &state,
        Method::POST,
        &accept,
        Some(&json!({ "changes": [0] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert_eq!(accepted["suggestion"]["state"], "accepted");
    assert_eq!(accepted["suggestion"]["acceptedChanges"], json!([0]));
    assert_eq!(
        accepted["page"]["body"],
        "- Stop the service and its queue\n- Rebuild\n- Start it"
    );
    assert_eq!(accepted["page"]["pendingSuggestions"], 1);
    let (status, again) = call(&state, Method::POST, &accept, Some(&json!({}))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(again["code"], "suggestion_decided");

    let dismiss = format!(
        "/api/v1/pages/{id}/suggestions/{}/dismiss",
        other.suggestion.id
    );
    let (status, dismissed) = call(
        &state,
        Method::POST,
        &dismiss,
        Some(&json!({ "reason": "The team already knows" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{dismissed}");
    assert_eq!(
        dismissed["suggestion"]["dismissalReason"],
        "The team already knows"
    );
    assert_eq!(dismissed["page"]["pendingSuggestions"], 0);

    let mut kinds = Vec::new();
    while let Ok(payload) = heard.try_recv() {
        assert!(
            !payload.contains("Check the logs"),
            "never what a suggestion says"
        );
        let event: Value = serde_json::from_str(&payload).expect("JSON");
        kinds.push(event["type"].as_str().expect("a type").to_owned());
    }
    assert!(kinds.contains(&"page.changed".to_owned()), "{kinds:?}");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_guest_reviews_no_suggestion_and_starts_no_review() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let id = create(&mut connection, None, "Runbook").await;
    let pending = suggest(
        &mut connection,
        id,
        None,
        adding("- Wait a minute"),
        elysia(),
    )
    .await
    .expect("suggested");
    set_role(&mut connection, "guest").await;
    let accept_pending = format!(
        "/api/v1/pages/{id}/suggestions/{}/accept",
        pending.suggestion.id
    );
    for (method, path) in [
        (Method::GET, format!("/api/v1/pages/{id}/suggestions")),
        (Method::POST, accept_pending),
        (Method::POST, "/api/v1/company-doc/review".to_owned()),
    ] {
        let (status, refused) = call(&state, method.clone(), &path, Some(&json!({}))).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
        assert_eq!(refused["code"], "members_only", "{method} {path}");
    }
    let still = page_suggestion::pending(&mut connection, id)
        .await
        .expect("read");
    assert_eq!(still.len(), 1, "nothing a guest sent was decided");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn the_tree_the_page_and_the_topbars_search_count_what_waits() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let id = create(&mut connection, None, "Ingest runbook").await;
    for line in ["- Stop the service", "- Tell the team"] {
        suggest(&mut connection, id, None, adding(line), elysia())
            .await
            .expect("suggested");
    }

    let (status, tree) = call(&state, Method::GET, "/api/v1/pages", None).await;
    assert_eq!(status, StatusCode::OK);
    let listed = tree["pages"]
        .as_array()
        .expect("pages")
        .iter()
        .find(|listed| listed["id"] == json!(id))
        .expect("listed");
    assert_eq!(listed["pendingSuggestions"], 2, "the tree says what waits");
    let (_, read) = call(&state, Method::GET, &format!("/api/v1/pages/{id}"), None).await;
    assert_eq!(read["page"]["pendingSuggestions"], 2);
    assert_eq!(read["page"]["companyDoc"], false);

    let (status, found) = call(&state, Method::GET, "/api/v1/search?q=ingest", None).await;
    assert_eq!(status, StatusCode::OK, "{found}");
    let hit = found["results"]
        .as_array()
        .expect("results")
        .iter()
        .find(|hit| hit["kind"] == "page" && hit["id"] == json!(id))
        .expect("found");
    assert_eq!(hit["pendingSuggestions"], 2);

    let (_, section) = call(
        &state,
        Method::GET,
        &format!("/api/v1/pages/{}", Section::Glossary.page_id()),
        None,
    )
    .await;
    assert_eq!(
        (
            &section["page"]["companyDoc"],
            &section["page"]["pendingSuggestions"]
        ),
        (&json!(true), &json!(0))
    );
}
