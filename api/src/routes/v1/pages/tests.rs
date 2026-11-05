// Copyright © 2026 Jalapeno Labs

//! Pages against Postgres (#196): publishing and its versions, restoring one, the tree's rules, the
//! knowledge index following each change in the same transaction, agents refused where they only
//! propose, and the routes refusing guests.
//!
//! Those are `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs
//! them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::Utc;
use diesel::QueryableByName;
use diesel::sql_types::{BigInt, Text, Uuid as SqlUuid};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use tokio::join;
use tower::ServiceExt as _;
use uuid::Uuid;

use super::page_edit;
use crate::models::agent::{Agent, Author};
use crate::models::knowledge_document;
use crate::models::page::{self, PageEdit, PageError, Placement, Recorded};
use crate::realtime::EventBus;
use crate::routes;
use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database, signed_in_cookie};

const ELYSIA_USER_ID: Uuid = crate::models::user::ELYSIA_USER_ID;

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

#[derive(QueryableByName)]
struct DocumentId {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
}

fn author(agent: Option<&Agent>) -> Author<'_> {
    Author {
        user_id: TEST_PERSON_ID,
        agent,
        at: Utc::now(),
    }
}

fn edit(title: &str, body: &str) -> PageEdit {
    PageEdit {
        title: title.to_owned(),
        body: body.to_owned(),
    }
}

async fn create(connection: &mut AsyncPgConnection, parent_id: Option<Uuid>, title: &str) -> Uuid {
    page::create(
        connection,
        parent_id,
        edit(title, "Its body."),
        author(None),
    )
    .await
    .expect("created")
    .page
    .id
}

/// How many indexed passages of page `id` match `query`, parsed as searches parse it.
async fn matches(connection: &mut AsyncPgConnection, id: Uuid, query: &str) -> i64 {
    diesel::sql_query(
        "SELECT count(*) AS count FROM knowledge_passages p \
         JOIN knowledge_documents d ON d.id = p.document_id \
         WHERE d.page_id = $1 AND d.source = 'page' \
           AND p.search_vector @@ websearch_to_tsquery('english', $2)",
    )
    .bind::<SqlUuid, _>(id)
    .bind::<Text, _>(query)
    .get_result::<Count>(connection)
    .await
    .expect("the search runs")
    .count
}

async fn is_indexed(connection: &mut AsyncPgConnection, id: Uuid) -> bool {
    diesel::sql_query("SELECT count(*) AS count FROM knowledge_documents WHERE page_id = $1")
        .bind::<SqlUuid, _>(id)
        .get_result::<Count>(connection)
        .await
        .expect("the count runs")
        .count
        == 1
}

async fn execute(connection: &mut AsyncPgConnection, statement: String) {
    diesel::sql_query(statement)
        .execute(connection)
        .await
        .expect("the statement runs");
}

/// The titles of `parent_id`'s live children, in order.
async fn children(connection: &mut AsyncPgConnection, parent_id: Option<Uuid>) -> Vec<String> {
    page::list(connection, false)
        .await
        .expect("listed")
        .into_iter()
        .filter(|page| page.parent_id == parent_id)
        .map(|page| page.title)
        .collect()
}

fn kinds(history: &[page::HistoryEntry]) -> Vec<&str> {
    history.iter().map(|entry| entry.kind.as_str()).collect()
}

fn refused<Expected>(outcome: &Result<Recorded, PageError>, expected: Expected) -> bool
where
    Expected: Fn(&PageError) -> bool,
{
    outcome.as_ref().is_err_and(expected)
}

#[test]
fn a_title_is_kept_on_one_line_and_a_bodys_ending_never_counts_as_a_change() {
    let edit =
        page_edit("  Deploying \n  the   API ", "First.\r\n\r\n    code\r\n\n").expect("valid");
    assert_eq!(edit.title, "Deploying the API");
    assert_eq!(
        edit.body, "First.\n\n    code",
        "the Markdown itself is untouched"
    );
}

#[test]
fn a_page_needs_a_title_but_may_be_empty() {
    assert_eq!(page_edit("Runbooks", "  \n").expect("valid").body, "");

    for (title, body, field) in [
        ("   ", "", "title"),
        (&*"t".repeat(301), "", "title"),
        ("Runbooks", &*"b".repeat(100_001), "body"),
        ("Run\0books", "", "title"),
        ("Runbooks", "A NUL\0 inside.", "body"),
    ] {
        let errors = page_edit(title, body).expect_err("refused");
        assert!(errors.field_errors().contains_key(field), "{field}");
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn publishing_makes_a_version_naming_who_and_their_agent_and_indexes_it_at_once() {
    let (_url, mut connection) = migrated_database().await;
    let agent = Agent::mcp_client("claude-code-client", "Claude Code").expect("recordable");
    let session = Agent::session(12);

    let created = page::create(
        &mut connection,
        None,
        edit("Deploying", "Run the WAY-123 script."),
        author(Some(&agent)),
    )
    .await
    .expect("created");
    let id = created.page.id;
    assert_eq!(created.page.current_version, 1);
    assert_eq!(matches(&mut connection, id, "WAY-123").await, 1);

    let published = page::publish(
        &mut connection,
        id,
        edit("Deploying", "Roll the fleet out by region."),
        1,
        author(Some(&session)),
    )
    .await
    .expect("published");
    assert_eq!(published.changed, [id]);
    assert_eq!(published.page.current_version, 2);
    assert_eq!(published.page.body, "Roll the fleet out by region.");

    assert_eq!(
        matches(&mut connection, id, "WAY-123").await,
        0,
        "the index holds what the page says now"
    );
    assert_eq!(matches(&mut connection, id, "fleet region").await, 1);

    let versions = page::versions(&mut connection, id).await.expect("listed");
    let published_by: Vec<(i32, Option<&str>)> = versions
        .iter()
        .map(|version| (version.version, version.agent.as_deref()))
        .collect();
    assert_eq!(
        published_by,
        [(2, Some("session:12")), (1, Some("mcp:claude-code-client"))],
        "newest first, each naming the agent acting for its person"
    );
    assert_eq!(versions[0].created_by, TEST_PERSON_ID);
    assert_eq!(versions[1].agent_name.as_deref(), Some("Claude Code"));

    let first = page::find_version(&mut connection, id, 1)
        .await
        .expect("any version reads");
    assert_eq!(first.body, "Run the WAY-123 script.");

    let history = page::history(&mut connection, id).await.expect("history");
    assert_eq!(kinds(&history), ["created", "published"]);
    assert_eq!(history[1].data, json!({ "version": 2 }));
    assert_eq!(history[1].agent.as_deref(), Some("session:12"));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_publish_from_an_older_version_is_refused_and_identical_text_makes_no_version() {
    let (_url, mut connection) = migrated_database().await;
    let id = create(&mut connection, None, "Deploying").await;
    page::publish(
        &mut connection,
        id,
        edit("Deploying", "Theirs."),
        1,
        author(None),
    )
    .await
    .expect("published");

    let stale = page::publish(
        &mut connection,
        id,
        edit("Deploying", "Mine."),
        1,
        author(None),
    )
    .await;
    assert!(refused(&stale, |error| matches!(error, PageError::Changed)));
    assert_eq!(
        page::find(&mut connection, id).await.expect("found").body,
        "Theirs.",
        "nothing is overwritten"
    );

    for base_version in [1, 2] {
        let unchanged = page::publish(
            &mut connection,
            id,
            edit("Deploying", "Theirs."),
            base_version,
            author(None),
        )
        .await
        .expect("nothing to refuse: it would overwrite nothing");
        assert!(unchanged.changed.is_empty());
    }
    assert_eq!(
        page::versions(&mut connection, id)
            .await
            .expect("listed")
            .len(),
        2,
        "identical text makes no version"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn two_publishes_at_once_never_share_a_version_and_the_second_is_told() {
    let (url, mut first) = migrated_database().await;
    let id = create(&mut first, None, "Deploying").await;
    let mut second = {
        use diesel_async::AsyncConnection;
        use secrecy::ExposeSecret;
        AsyncPgConnection::establish(url.expose_secret())
            .await
            .expect("a second connection")
    };

    let (one, other) = join!(
        page::publish(&mut first, id, edit("Deploying", "One."), 1, author(None)),
        page::publish(
            &mut second,
            id,
            edit("Deploying", "Other."),
            1,
            author(None)
        ),
    );

    let outcomes = [one, other];
    let published = outcomes.iter().filter(|outcome| outcome.is_ok()).count();
    assert_eq!(published, 1, "one of them wins");
    assert!(
        outcomes
            .iter()
            .any(|outcome| refused(outcome, |error| matches!(error, PageError::Changed)))
    );
    assert_eq!(
        page::versions(&mut first, id).await.expect("listed").len(),
        2
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn restoring_a_version_publishes_it_again_and_loses_nothing() {
    let (_url, mut connection) = migrated_database().await;
    let id = create(&mut connection, None, "First title").await;
    page::publish(
        &mut connection,
        id,
        edit("Second title", "Second body."),
        1,
        author(None),
    )
    .await
    .expect("published");

    let restored = page::restore_version(&mut connection, id, 1, author(None))
        .await
        .expect("restored");
    assert_eq!(restored.page.current_version, 3);
    assert_eq!(restored.page.title, "First title");
    assert_eq!(restored.page.body, "Its body.");
    assert_eq!(matches(&mut connection, id, "second").await, 0);
    assert_eq!(matches(&mut connection, id, "first").await, 1);

    let newest = page::find_version(&mut connection, id, 3)
        .await
        .expect("version 3 reads");
    assert_eq!(newest.summary.restored_from, Some(1));
    let history = page::history(&mut connection, id).await.expect("history");
    assert_eq!(
        history.last().expect("entries").data,
        json!({ "version": 3, "restoredFrom": 1 })
    );

    let again = page::restore_version(&mut connection, id, 3, author(None))
        .await
        .expect("a version the page holds is nothing to do");
    assert!(again.changed.is_empty());

    page::find_version(&mut connection, id, 2)
        .await
        .expect("the version it replaced stays");
    let unknown = page::restore_version(&mut connection, id, 9, author(None)).await;
    assert!(refused(&unknown, |error| matches!(
        error,
        PageError::Database(diesel::result::Error::NotFound)
    )));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_publish_keeps_a_pages_learning_fields_in_its_versions_and_its_index() {
    let (_url, mut connection) = migrated_database().await;
    let id = create(&mut connection, None, "Pin every tool").await;
    execute(
        &mut connection,
        format!(
            "UPDATE pages SET category = 'convention', evidence = 'CI drifted on WAY-123.' \
             WHERE id = '{id}'"
        ),
    )
    .await;

    let published = page::publish(
        &mut connection,
        id,
        edit("Pin every tool", "Never use latest."),
        1,
        author(None),
    )
    .await
    .expect("published");

    assert_eq!(published.page.category.as_deref(), Some("convention"));
    let version = page::find_version(&mut connection, id, 2)
        .await
        .expect("reads");
    assert_eq!(version.category.as_deref(), Some("convention"));
    assert_eq!(version.evidence.as_deref(), Some("CI drifted on WAY-123."));
    assert_eq!(
        matches(&mut connection, id, "WAY-123").await,
        1,
        "evidence is indexed as a learning's is"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_page_opens_from_the_index_by_its_id() {
    let (_url, mut connection) = migrated_database().await;
    let id = create(&mut connection, None, "Deploying").await;
    let document_id = diesel::sql_query("SELECT id FROM knowledge_documents WHERE page_id = $1")
        .bind::<SqlUuid, _>(id)
        .get_result::<DocumentId>(&mut connection)
        .await
        .expect("indexed")
        .id;

    let document = knowledge_document::get(&mut connection, document_id)
        .await
        .expect("reads")
        .expect("found");

    assert_eq!(document.citation.source, "page");
    assert_eq!(document.citation.title, "Deploying");
    assert_eq!(document.citation.url, None);
    assert_eq!(document.citation.person.id, TEST_PERSON_ID);
    assert_eq!(
        serde_json::to_value(&document.origin).expect("serializes"),
        json!({ "kind": "page", "pageId": id })
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn pages_append_in_order_and_move_with_what_is_under_them() {
    let (_url, mut connection) = migrated_database().await;
    let handbook = create(&mut connection, None, "Handbook").await;
    let runbooks = create(&mut connection, None, "Runbooks").await;
    let deploying = create(&mut connection, Some(runbooks), "Deploying").await;
    let rollback = create(&mut connection, Some(deploying), "Rolling back").await;
    assert_eq!(
        children(&mut connection, None).await,
        ["Handbook", "Runbooks"]
    );

    let placement = Placement {
        parent_id: None,
        after: None,
    };
    let moved = page::move_page(&mut connection, runbooks, placement, author(None))
        .await
        .expect("moved first");
    assert_eq!(moved.changed, [runbooks]);
    assert_eq!(
        children(&mut connection, None).await,
        ["Runbooks", "Handbook"]
    );

    let placement = Placement {
        parent_id: Some(handbook),
        after: None,
    };
    page::move_page(&mut connection, deploying, placement, author(None))
        .await
        .expect("moved under another page");
    assert_eq!(
        children(&mut connection, Some(handbook)).await,
        ["Deploying"]
    );
    assert_eq!(
        children(&mut connection, Some(deploying)).await,
        ["Rolling back"],
        "what was under it comes along"
    );
    assert_eq!(
        page::find(&mut connection, rollback)
            .await
            .expect("found")
            .parent_id,
        Some(deploying)
    );

    let history = page::history(&mut connection, deploying)
        .await
        .expect("history");
    assert_eq!(kinds(&history), ["created", "moved"]);
    assert_eq!(
        history[1].data,
        json!({
            "from": { "parentId": runbooks, "after": null },
            "to": { "parentId": handbook, "after": null },
        })
    );

    let stays = Placement {
        parent_id: Some(handbook),
        after: None,
    };
    let unmoved = page::move_page(&mut connection, deploying, stays, author(None))
        .await
        .expect("a page already there is nothing to do");
    assert!(unmoved.changed.is_empty());
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_page_cannot_move_under_itself_a_deleted_parent_or_a_sibling_that_left() {
    let (_url, mut connection) = migrated_database().await;
    let top = create(&mut connection, None, "Top").await;
    let middle = create(&mut connection, Some(top), "Middle").await;
    let bottom = create(&mut connection, Some(middle), "Bottom").await;
    let other = create(&mut connection, None, "Other").await;

    for parent_id in [top, middle, bottom] {
        let placement = Placement {
            parent_id: Some(parent_id),
            after: None,
        };
        let refusal = page::move_page(&mut connection, top, placement, author(None)).await;
        assert!(refused(&refusal, |error| matches!(
            error,
            PageError::UnderItself
        )));
    }

    let elsewhere = Placement {
        parent_id: Some(other),
        after: Some(bottom),
    };
    let refusal = page::move_page(&mut connection, middle, elsewhere, author(None)).await;
    assert!(refused(&refusal, |error| matches!(
        error,
        PageError::PlacementGone
    )));

    page::delete(&mut connection, other, author(None))
        .await
        .expect("deleted");
    let under_deleted = Placement {
        parent_id: Some(other),
        after: None,
    };
    let refusal = page::move_page(&mut connection, middle, under_deleted, author(None)).await;
    assert!(refused(&refusal, |error| matches!(
        error,
        PageError::ParentGone
    )));
    let created = page::create(
        &mut connection,
        Some(other),
        edit("Child", ""),
        author(None),
    )
    .await;
    assert!(refused(&created, |error| matches!(
        error,
        PageError::ParentGone
    )));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn two_pages_moved_under_each_other_at_once_never_make_a_cycle() {
    let (url, mut first) = migrated_database().await;
    let left = create(&mut first, None, "Left").await;
    let right = create(&mut first, None, "Right").await;
    let mut second = {
        use diesel_async::AsyncConnection;
        use secrecy::ExposeSecret;
        AsyncPgConnection::establish(url.expose_secret())
            .await
            .expect("a second connection")
    };

    let under_right = Placement {
        parent_id: Some(right),
        after: None,
    };
    let under_left = Placement {
        parent_id: Some(left),
        after: None,
    };
    let (one, other) = join!(
        page::move_page(&mut first, left, under_right, author(None)),
        page::move_page(&mut second, right, under_left, author(None)),
    );

    let outcomes = [one, other];
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_ok()).count(),
        1,
        "the tree lock takes them one at a time, so the second sees the first"
    );
    assert!(
        outcomes
            .iter()
            .any(|outcome| refused(outcome, |error| matches!(error, PageError::UnderItself)))
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn reordering_writes_one_row_until_a_spot_runs_out_of_room_and_keeps_the_order_after() {
    let (_url, mut connection) = migrated_database().await;
    let first = create(&mut connection, None, "First").await;
    create(&mut connection, None, "Last").await;

    // Each lands right after First, halving the room there, until none is left.
    let mut expected = vec!["First".to_owned(), "Last".to_owned()];
    let mut renumbered = false;
    for number in 0..24 {
        let title = format!("Page {number}");
        let page_id = create(&mut connection, None, &title).await;
        let placement = Placement {
            parent_id: None,
            after: Some(first),
        };
        let moved = page::move_page(&mut connection, page_id, placement, author(None))
            .await
            .expect("moved");
        expected.insert(1, title);
        renumbered |= moved.changed.len() > 1;
        assert_eq!(children(&mut connection, None).await, expected);
    }

    assert!(renumbered, "the spot ran out of room at least once");
    let positions: Vec<i64> = page::list(&mut connection, false)
        .await
        .expect("listed")
        .into_iter()
        .map(|page| page.position)
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn deleting_takes_what_is_under_a_page_and_restoring_brings_back_what_it_took() {
    let (_url, mut connection) = migrated_database().await;
    let top = create(&mut connection, None, "Top").await;
    let kept = create(&mut connection, Some(top), "Kept").await;
    let earlier = create(&mut connection, Some(top), "Deleted earlier").await;
    let below = create(&mut connection, Some(kept), "Below").await;

    page::delete(&mut connection, earlier, author(None))
        .await
        .expect("deleted on its own");
    let deleted = page::delete(&mut connection, top, author(None))
        .await
        .expect("deleted");
    let mut changed = deleted.changed.clone();
    changed.sort();
    let mut expected = vec![top, kept, below];
    expected.sort();
    assert_eq!(changed, expected, "the page deleted earlier is not changed");
    for id in [top, kept, below] {
        assert!(!is_indexed(&mut connection, id).await);
    }
    assert!(children(&mut connection, None).await.is_empty());
    assert_eq!(
        page::list(&mut connection, true)
            .await
            .expect("listed")
            .len(),
        4
    );

    let refusal = page::restore(&mut connection, kept, author(None)).await;
    assert!(refused(&refusal, |error| matches!(
        error,
        PageError::ParentGone
    )));
    let refusal = page::publish(&mut connection, kept, edit("Kept", "New."), 1, author(None)).await;
    assert!(refused(&refusal, |error| matches!(
        error,
        PageError::Deleted
    )));

    let restored = page::restore(&mut connection, top, author(None))
        .await
        .expect("restored");
    assert_eq!(restored.changed.len(), 3);
    assert_eq!(children(&mut connection, Some(top)).await, ["Kept"]);
    assert_eq!(children(&mut connection, Some(kept)).await, ["Below"]);
    for id in [top, kept, below] {
        assert!(is_indexed(&mut connection, id).await);
    }
    assert!(
        !is_indexed(&mut connection, earlier).await,
        "what was deleted on its own stays deleted"
    );

    let history = page::history(&mut connection, kept).await.expect("history");
    assert_eq!(kinds(&history), ["created", "deleted", "restored"]);
    assert_eq!(history[1].data, json!({ "deletedWith": top }));
    assert_eq!(history[2].data["restoredWith"], json!(top));

    page::restore(&mut connection, earlier, author(None))
        .await
        .expect("its parent is live again");
    assert_eq!(
        children(&mut connection, Some(top)).await,
        ["Kept", "Deleted earlier"],
        "it comes back where it was"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_page_appended_while_another_is_deleted_never_takes_its_place() {
    let (_url, mut connection) = migrated_database().await;
    create(&mut connection, None, "First").await;
    let second = create(&mut connection, None, "Second").await;

    page::delete(&mut connection, second, author(None))
        .await
        .expect("deleted");
    create(&mut connection, None, "Third").await;

    page::restore(&mut connection, second, author(None))
        .await
        .expect("nothing took its place");
    assert_eq!(
        children(&mut connection, None).await,
        ["First", "Second", "Third"]
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn agents_only_propose_to_a_proposal_only_page_and_people_still_edit_it() {
    let (_url, mut connection) = migrated_database().await;
    let parent = create(&mut connection, None, "Company").await;
    let id = create(&mut connection, Some(parent), "Glossary").await;
    execute(
        &mut connection,
        format!("UPDATE pages SET proposal_only = TRUE WHERE id = '{id}'"),
    )
    .await;

    let client = Agent::mcp_client("claude-code-client", "Claude Code").expect("recordable");
    let session = Agent::session(12);
    let elysia = Author {
        user_id: ELYSIA_USER_ID,
        agent: None,
        at: Utc::now(),
    };
    for agent_author in [author(Some(&client)), author(Some(&session)), elysia] {
        let published = page::publish(
            &mut connection,
            id,
            edit("Glossary", "An agent's change."),
            1,
            agent_author,
        )
        .await;
        assert!(refused(&published, |error| matches!(
            error,
            PageError::ProposalOnly
        )));
        let restored = page::restore_version(&mut connection, id, 1, agent_author).await;
        assert!(
            refused(&restored, |error| matches!(error, PageError::ProposalOnly)),
            "a restore is a write of its content"
        );
        let placement = Placement {
            parent_id: None,
            after: None,
        };
        let moved = page::move_page(&mut connection, id, placement, agent_author).await;
        assert!(refused(&moved, |error| matches!(
            error,
            PageError::ProposalOnly
        )));
        let deleted = page::delete(&mut connection, parent, agent_author).await;
        assert!(
            refused(&deleted, |error| matches!(error, PageError::ProposalOnly)),
            "deleting a parent would take it"
        );
    }
    assert_eq!(children(&mut connection, None).await, ["Company"]);

    let published = page::publish(
        &mut connection,
        id,
        edit("Glossary", "A person's change."),
        1,
        author(None),
    )
    .await
    .expect("a person edits it directly");
    assert_eq!(published.page.current_version, 2);
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

async fn set_role(connection: &mut AsyncPgConnection, role: &str) {
    execute(
        connection,
        format!("UPDATE users SET role = '{role}' WHERE id = '{TEST_PERSON_ID}'"),
    )
    .await;
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn members_create_publish_read_and_restore_pages_over_the_routes() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    set_role(&mut connection, "member").await;

    let (status, created) = call(
        &state,
        Method::POST,
        "/api/v1/pages",
        Some(&json!({ "title": "  Deploying   the   API ", "body": "First.\r\n\r\nSecond.\n\n" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let page = &created["page"];
    let id = page["id"].as_str().expect("an id");
    assert_eq!(page["title"], "Deploying the API");
    assert_eq!(
        page["body"], "First.\n\nSecond.",
        "line endings normalized and trailing whitespace dropped"
    );
    assert_eq!(page["currentVersion"], 1);
    assert_eq!(page["parentId"], Value::Null);
    assert_eq!(page["publishedBy"], json!(TEST_PERSON_ID));
    assert_eq!(page["agent"], Value::Null);

    let publish = format!("/api/v1/pages/{id}/publish");
    let (status, published) = call(
        &state,
        Method::POST,
        &publish,
        Some(&json!({ "baseVersion": 1, "title": "Deploying the API", "body": "Revised.\n" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{published}");
    assert_eq!(published["outcome"], "published");
    assert_eq!(published["page"]["currentVersion"], 2);

    let (status, conflict) = call(
        &state,
        Method::POST,
        &publish,
        Some(&json!({ "baseVersion": 1, "title": "Deploying the API", "body": "Mine." })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["code"], "page_changed");

    let (status, unchanged) = call(
        &state,
        Method::POST,
        &publish,
        Some(&json!({ "baseVersion": 2, "title": "Deploying the API", "body": "Revised.\r\n" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unchanged["outcome"], "unchanged");

    let (status, restored) = call(
        &state,
        Method::POST,
        &format!("/api/v1/pages/{id}/versions/1/restore"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["page"]["currentVersion"], 3);
    assert_eq!(restored["page"]["body"], "First.\n\nSecond.");

    let (_, versions) = call(
        &state,
        Method::GET,
        &format!("/api/v1/pages/{id}/versions"),
        None,
    )
    .await;
    assert_eq!(versions["versions"][0]["version"], 3);
    assert_eq!(versions["versions"][0]["restoredFrom"], 1);
    assert!(versions["versions"][0].get("body").is_none());

    let (status, version) = call(
        &state,
        Method::GET,
        &format!("/api/v1/pages/{id}/versions/2"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{version}");
    assert_eq!(version["version"]["body"], "Revised.");
    assert_eq!(version["version"]["version"], 2);

    let (status, _) = call(
        &state,
        Method::GET,
        &format!("/api/v1/pages/{id}/versions/9"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, listed) = call(&state, Method::GET, "/api/v1/pages", None).await;
    assert_eq!(listed["pages"][0]["title"], "Deploying the API");
    assert!(listed["pages"][0].get("body").is_none());
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn members_move_delete_and_restore_pages_over_the_routes() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let id = create(&mut connection, None, "Deploying the API").await;
    set_role(&mut connection, "member").await;

    let (status, moved) = call(
        &state,
        Method::POST,
        &format!("/api/v1/pages/{id}/move"),
        Some(&json!({ "parentId": id, "after": null })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(moved["code"], "page_under_itself");

    let (status, _) = call(&state, Method::DELETE, &format!("/api/v1/pages/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, deleted) = call(&state, Method::GET, &format!("/api/v1/pages/{id}"), None).await;
    assert_eq!(status, StatusCode::OK, "a deleted page still reads");
    assert!(deleted["page"]["deletedAt"].is_string());
    let (_, listed) = call(&state, Method::GET, "/api/v1/pages?deleted=true", None).await;
    assert_eq!(listed["pages"][0]["id"], json!(id));

    let (status, refused) = call(
        &state,
        Method::POST,
        &format!("/api/v1/pages/{id}/publish"),
        Some(&json!({ "baseVersion": 1, "title": "Deploying the API", "body": "After." })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["code"], "page_deleted");

    let (status, restored) = call(
        &state,
        Method::POST,
        &format!("/api/v1/pages/{id}/restore"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["page"]["deletedAt"], Value::Null);

    let (_, history) = call(
        &state,
        Method::GET,
        &format!("/api/v1/pages/{id}/history"),
        None,
    )
    .await;
    let recorded: Vec<&str> = history["history"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|entry| entry["kind"].as_str().expect("a kind"))
        .collect();
    assert_eq!(recorded, ["created", "deleted", "restored"]);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn every_page_a_change_touched_is_announced_by_its_id_alone() {
    let (url, mut connection) = migrated_database().await;
    let mut state = app_state(&url).await;
    state.events = EventBus::in_process();
    let mut heard = state.events.subscribe();
    let top = create(&mut connection, None, "Top").await;
    let child = create(&mut connection, Some(top), "Child").await;

    let publish = format!("/api/v1/pages/{top}/publish");
    let body = json!({ "baseVersion": 1, "title": "Top", "body": "Secret words." });
    call(&state, Method::POST, &publish, Some(&body)).await;
    call(&state, Method::POST, &publish, Some(&body)).await;
    call(
        &state,
        Method::DELETE,
        &format!("/api/v1/pages/{top}"),
        None,
    )
    .await;

    let mut announced = Vec::new();
    while let Ok(payload) = heard.try_recv() {
        assert!(!payload.contains("Secret words"), "never what a page says");
        let event: Value = serde_json::from_str(&payload).expect("JSON");
        if event["type"] == "page.changed" {
            announced.push(event["data"].clone());
        }
    }
    let mut deleted = vec![json!({ "id": top }), json!({ "id": child })];
    deleted.sort_by_key(ToString::to_string);
    announced[1..].sort_by_key(ToString::to_string);
    assert_eq!(
        announced[0],
        json!({ "id": top }),
        "the publish that changed it"
    );
    assert_eq!(
        announced[1..],
        deleted,
        "the same text again is silent, and a deletion names everything it took"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_guest_reaches_no_page() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let id = create(&mut connection, None, "Deploying").await;
    set_role(&mut connection, "guest").await;

    let create_body = json!({ "title": "A page" });
    let publish_body = json!({ "baseVersion": 1, "title": "A page", "body": "" });
    let move_body = json!({ "parentId": null, "after": null });
    for (method, path, body) in [
        (Method::GET, "/api/v1/pages".to_owned(), None),
        (Method::POST, "/api/v1/pages".to_owned(), Some(&create_body)),
        (Method::GET, format!("/api/v1/pages/{id}"), None),
        (Method::DELETE, format!("/api/v1/pages/{id}"), None),
        (
            Method::POST,
            format!("/api/v1/pages/{id}/publish"),
            Some(&publish_body),
        ),
        (
            Method::POST,
            format!("/api/v1/pages/{id}/move"),
            Some(&move_body),
        ),
        (Method::POST, format!("/api/v1/pages/{id}/restore"), None),
        (Method::GET, format!("/api/v1/pages/{id}/history"), None),
        (Method::GET, format!("/api/v1/pages/{id}/versions"), None),
        (Method::GET, format!("/api/v1/pages/{id}/versions/1"), None),
        (
            Method::POST,
            format!("/api/v1/pages/{id}/versions/1/restore"),
            None,
        ),
    ] {
        let (status, refused) = call(&state, method.clone(), &path, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
        assert_eq!(refused["code"], "members_only", "{method} {path}");
    }
    assert_eq!(
        page::find(&mut connection, id)
            .await
            .expect("found")
            .current_version,
        1,
        "nothing a guest sent was written"
    );
}
