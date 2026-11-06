// Copyright © 2026 Jalapeno Labs

//! Finding pages against Postgres (#204): the knowledge search scoped to pages, with learnings and
//! the company doc among them, narrowed by label, author, and date; each result naming the page
//! that opens it, and two pages saying the same thing staying two results; a passage read back to
//! be found in a page's Markdown; and titles found as they are typed, by members alone.
//!
//! Those are `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs
//! them.

use axum::http::StatusCode;
use chrono::{Duration, Utc};
use serde_json::Value;
use uuid::Uuid;

use super::{ELYSIA_USER_ID, author, create, edit, set_role};
use crate::knowledge::passages::{PASSAGE_TARGET_CHARACTERS, split};
use crate::models::agent::Author;
use crate::models::company_doc::Section;
use crate::models::learning::{self, LearningContent};
use crate::models::page::{self, LEARNINGS_PAGE_ID};
use crate::models::page_label::{self, Label};
use crate::routes::v1::knowledge::tests::get;
use crate::state::AppState;
use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database};

/// The sources a search of pages names.
const PAGES: &str = "source=page,learning,company_doc";

async fn results(state: &AppState, query: &str) -> Vec<Value> {
    let (status, answer) = get(state, &format!("/api/v1/knowledge/search?{query}")).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    answer["results"].as_array().expect("results").clone()
}

/// The pages a search answers, by id, best first.
async fn pages_found(state: &AppState, query: &str) -> Vec<Uuid> {
    results(state, query)
        .await
        .iter()
        .map(|result| {
            serde_json::from_value(result["citation"]["pageId"].clone()).expect("a page's id")
        })
        .collect()
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_search_of_pages_finds_learnings_and_the_company_doc_and_opens_each_by_its_page() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;

    let ordinary = page::create(
        &mut connection,
        None,
        edit("Deploying", "How we ship the zeppelin."),
        author(None),
    )
    .await
    .expect("created")
    .page
    .id;
    let saved = learning::save(
        &mut connection,
        LearningContent {
            title: "Zeppelin ballast".to_owned(),
            body: "The zeppelin needs ballast before it ships.".to_owned(),
            category: "procedure".to_owned(),
            evidence: "It floated off.".to_owned(),
        },
        author(None),
    )
    .await
    .expect("saved")
    .learning
    .id;
    let section = Section::Glossary.page_id();
    let current = page::find(&mut connection, section).await.expect("found");
    page::publish(
        &mut connection,
        section,
        edit(&current.title, "Zeppelin: our word for the release train."),
        current.current_version,
        author(None),
    )
    .await
    .expect("published");

    let mut found = pages_found(&state, &format!("q=zeppelin&{PAGES}")).await;
    found.sort();
    let mut expected = vec![ordinary, saved, section];
    expected.sort();
    assert_eq!(
        found, expected,
        "all three are pages, and each names its own"
    );

    let only_pages = pages_found(&state, "q=zeppelin&source=page").await;
    assert_eq!(only_pages, [ordinary], "the filter can still pick one kind");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_search_of_pages_narrows_by_label_author_and_date() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;

    let mine = create(&mut connection, None, "Zeppelin mine").await;
    let long_ago = Utc::now() - Duration::days(30);
    let hers = page::create(
        &mut connection,
        None,
        edit("Zeppelin hers", "Its body."),
        Author {
            user_id: ELYSIA_USER_ID,
            at: long_ago,
            ..author(None)
        },
    )
    .await
    .expect("created")
    .page
    .id;
    let label = Label::parse("runbook").expect("a label");
    page_label::add(&mut connection, hers, &label, author(None))
        .await
        .expect("labeled");

    let search = format!("q=zeppelin&{PAGES}");
    assert_eq!(
        pages_found(&state, &format!("{search}&label=Runbook")).await,
        [hers],
    );
    assert_eq!(
        pages_found(&state, &format!("{search}&person={ELYSIA_USER_ID}")).await,
        [hers],
        "a page's author is who published its newest version, a machine included",
    );
    assert_eq!(
        pages_found(&state, &format!("{search}&person={TEST_PERSON_ID}")).await,
        [mine],
    );
    let week_ago = (Utc::now() - Duration::days(7))
        .to_rfc3339()
        .replace('+', "%2B");
    assert_eq!(
        pages_found(&state, &format!("{search}&from={week_ago}")).await,
        [mine],
    );
    assert_eq!(
        pages_found(&state, &format!("{search}&to={week_ago}")).await,
        [hers],
    );

    // Publishing makes whoever did the page's author, and dates it now.
    let current = page::find(&mut connection, hers).await.expect("found");
    page::publish(
        &mut connection,
        hers,
        edit(&current.title, "Rewritten."),
        current.current_version,
        author(None),
    )
    .await
    .expect("published");
    assert!(
        pages_found(&state, &format!("{search}&person={ELYSIA_USER_ID}"))
            .await
            .is_empty(),
        "the filter matches the byline: the last to publish",
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn two_pages_saying_the_same_thing_are_two_results_with_no_copies() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let parent = create(&mut connection, None, "Parent").await;
    let first = create(&mut connection, None, "Zeppelin overview").await;
    let second = create(&mut connection, Some(parent), "Zeppelin overview").await;

    let found = results(&state, &format!("q=zeppelin&{PAGES}")).await;

    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0]["alsoRecordedBy"], serde_json::json!([]));
    let mut pages = pages_found(&state, &format!("q=zeppelin&{PAGES}")).await;
    pages.sort();
    let mut expected = vec![first, second];
    expected.sort();
    assert_eq!(pages, expected);

    let document = found[0]["citation"]["documentId"].as_str().expect("an id");
    let (status, opened) = get(&state, &format!("/api/v1/knowledge/documents/{document}")).await;
    assert_eq!(status, StatusCode::OK, "{opened}");
    assert_eq!(opened["document"]["alsoRecordedBy"], serde_json::json!([]));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_passage_reads_back_as_the_search_cited_it_and_nothing_else_is_found() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let paragraph = "word ".repeat(PASSAGE_TARGET_CHARACTERS / 4);
    let body = format!("{paragraph}\n\nThe zeppelin lands here.");
    let id = page::create(&mut connection, None, edit("Long", &body), author(None))
        .await
        .expect("created")
        .page
        .id;
    let cut = split(&format!("Long\n\n{body}"));
    let last = cut.len() - 1;
    assert!(last > 0, "the page spans passages");

    let found = results(&state, &format!("q=zeppelin&{PAGES}")).await;
    assert_eq!(found[0]["passages"][0]["position"], last);

    let (status, read) = get(&state, &format!("/api/v1/pages/{id}/passages/{last}")).await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["passage"]["position"], last);
    assert_eq!(read["passage"]["content"], cut[last]);

    let past_the_end = cut.len();
    let (status, _) = get(
        &state,
        &format!("/api/v1/pages/{id}/passages/{past_the_end}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = get(&state, &format!("/api/v1/pages/{id}/passages/nowhere")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    page::delete(&mut connection, id, author(None), None)
        .await
        .expect("deleted");
    let (status, _) = get(&state, &format!("/api/v1/pages/{id}/passages/0")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a deleted page leaves the index"
    );

    set_role(&mut connection, "guest").await;
    let (status, refused) = get(&state, &format!("/api/v1/pages/{id}/passages/0")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(refused["code"], "members_only");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn the_global_search_finds_titles_as_they_are_typed_for_members_alone() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let live = create(&mut connection, None, "Zeppelin runbook").await;
    create(&mut connection, None, "Runbooks elsewhere").await;
    let gone = create(&mut connection, None, "Runbook gone").await;
    page::delete(&mut connection, gone, author(None), None)
        .await
        .expect("deleted");
    assert!(
        page::search(
            &mut connection,
            &crate::search::SearchTerms::parse("gone"),
            5
        )
        .await
        .expect("searched")
        .is_empty(),
        "a deleted page is off the tree and out of the search",
    );

    let (status, answer) = get(&state, "/api/v1/search?q=zep%20runb").await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let pages: Vec<&Value> = answer["results"]
        .as_array()
        .expect("results")
        .iter()
        .filter(|hit| hit["kind"] == "page")
        .collect();
    assert_eq!(pages.len(), 1, "{answer}");
    assert_eq!(pages[0]["id"], live.to_string());
    assert_eq!(pages[0]["title"], "Zeppelin runbook");

    let (_, answer) = get(&state, "/api/v1/search?q=learn").await;
    let fixed = answer["results"]
        .as_array()
        .expect("results")
        .iter()
        .any(|hit| hit["id"] == LEARNINGS_PAGE_ID.to_string());
    assert!(fixed, "every page has a title to be found by: {answer}");

    set_role(&mut connection, "guest").await;
    let (status, answer) = get(&state, "/api/v1/search?q=zeppelin").await;
    assert_eq!(status, StatusCode::OK, "a guest still searches the rest");
    assert_eq!(
        answer["results"],
        serde_json::json!([]),
        "and finds no page"
    );
}
