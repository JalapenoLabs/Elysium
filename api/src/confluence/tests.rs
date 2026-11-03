// Copyright © 2026 Jalapeno Labs

//! [`Confluence`] end to end, against a local fake of Confluence Cloud's REST API.
//!
//! The fake answers the way Atlassian documents: HTTP basic auth with `401` for the wrong
//! credentials, v2's cursor paging for spaces, and v1's content search with expanded bodies,
//! paged by `_links.next`. It records every request, so tests can assert the exact CQL that
//! left the process. No live Confluence site exists here, so these prove Elysium's half of the
//! contract against the documented shapes, not against Atlassian.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode as AxumStatus, Uri};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use secrecy::SecretString;
use serde_json::{Value, json};

use super::*;

/// The account and token the fake accepts; anything else is answered `401`.
pub(crate) const EMAIL: &str = "alex@example.com";
pub(crate) const TOKEN: &str = "ATATTsecret-token-value";

/// The site an account names. Requests go to the fake, but every link Elysium builds for a
/// person still points here.
pub(crate) const SITE_URL: &str = "https://acme.atlassian.net";

/// The fake account's id, which its mentions name.
pub(crate) const ACCOUNT_ID: &str = "5b10a2844c20165700ede21g";

/// Someone else, who writes most of the fake's content.
pub(crate) const COLLEAGUE_ID: &str = "70121:aaaa-bbbb";

/// One request the fake received.
#[derive(Debug, Clone)]
pub(crate) struct Received {
    pub(crate) path: String,
    pub(crate) query: String,
}

pub(crate) type Log = Arc<Mutex<Vec<Received>>>;

/// A page in the space with `key`, changed by `by` at version `version`.
pub(crate) fn page(id: &str, key: &str, version: i64, by: &str, storage: &str) -> Value {
    json!({
        "id": id,
        "type": "page",
        "title": format!("Page {id}"),
        "space": { "key": key, "name": format!("Space {key}") },
        "version": {
            "number": version,
            "when": format!("2026-11-02T10:0{version}:00.000Z"),
            "by": { "accountId": by, "displayName": "Grace Hopper" },
        },
        "history": { "createdBy": { "accountId": by, "displayName": "Grace Hopper" } },
        "body": { "storage": { "value": storage, "representation": "storage" } },
        "_links": { "webui": format!("/spaces/{key}/pages/{id}") },
    })
}

/// A comment in the space with `key`, on the page `page_id` that `page_creator` created.
pub(crate) fn comment(
    id: &str,
    key: &str,
    page_id: &str,
    page_creator: &str,
    storage: &str,
) -> Value {
    json!({
        "id": id,
        "type": "comment",
        "title": format!("Re: Page {page_id}"),
        "space": { "key": key, "name": format!("Space {key}") },
        "version": {
            "number": 1,
            "when": "2026-11-02T11:00:00.000Z",
            "by": { "accountId": COLLEAGUE_ID, "displayName": "Grace Hopper" },
        },
        "history": { "createdBy": { "accountId": COLLEAGUE_ID } },
        "body": { "storage": { "value": storage } },
        "container": {
            "id": page_id,
            "type": "page",
            "title": format!("Page {page_id}"),
            "history": { "createdBy": { "accountId": page_creator } },
        },
        "_links": { "webui": format!("/spaces/{key}/pages/{page_id}?focusedCommentId={id}") },
    })
}

/// What the fake's search answers: two pages of results, the second in a space no connector
/// here picked unless it reads every space.
fn search_results(cursor: Option<&str>) -> Value {
    match cursor {
        None => json!({
            "results": [
                page("101", "ENG", 2, COLLEAGUE_ID, "<p>Launch on <strong>Friday</strong>.</p>"),
                comment("201", "ENG", "101", ACCOUNT_ID, "<p>Can we move it to Monday?</p>"),
            ],
            "_links": { "next": "/rest/api/content/search?cursor=page-two&limit=50" },
        }),
        Some(_) => json!({
            "results": [
                page("102", "SECRET", 1, COLLEAGUE_ID, "<p>Not for Elysium.</p>"),
            ],
            "_links": {},
        }),
    }
}

async fn fake_confluence(State(log): State<Log>, headers: HeaderMap, uri: Uri) -> Response {
    log.lock().expect("lock").push(Received {
        path: uri.path().to_owned(),
        query: uri.query().unwrap_or_default().to_owned(),
    });

    let expected = format!("Basic {}", BASE64.encode(format!("{EMAIL}:{TOKEN}")));
    let authorized = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        == Some(expected.as_str());
    if !authorized {
        return (
            AxumStatus::UNAUTHORIZED,
            axum::Json(json!({ "statusCode": 401, "message": "Unauthorized" })),
        )
            .into_response();
    }

    let query = uri.query().unwrap_or_default();
    match uri.path() {
        "/wiki/rest/api/user/current" => axum::Json(json!({
            "type": "known",
            "accountId": ACCOUNT_ID,
            "displayName": "Alex Navarro",
            "email": EMAIL,
        }))
        .into_response(),
        "/wiki/api/v2/spaces" if query.contains("cursor=second") => axum::Json(json!({
            "results": [{ "id": "3", "key": "~5b10", "name": "Alex's space" }],
            "_links": {},
        }))
        .into_response(),
        "/wiki/api/v2/spaces" => axum::Json(json!({
            "results": [
                { "id": "1", "key": "ENG", "name": "Engineering" },
                { "id": "2", "key": "SECRET", "name": "Secret" },
            ],
            "_links": { "next": "/wiki/api/v2/spaces?cursor=second&limit=250" },
        }))
        .into_response(),
        "/wiki/rest/api/content/search" => {
            let cursor = query.contains("cursor=page-two").then_some("page-two");
            axum::Json(search_results(cursor)).into_response()
        }
        _ => (AxumStatus::NOT_FOUND, "no such route").into_response(),
    }
}

/// A [`Confluence`] pointed at a fresh fake, and the log of what the fake was sent.
pub(crate) async fn fake_confluence_site() -> (Confluence, Log) {
    let log = Log::default();
    let router = Router::new()
        .fallback(fake_confluence)
        .with_state(Arc::clone(&log));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move { axum::serve(listener, router).await });
    let confluence =
        Confluence::with_test_origin(reqwest::Client::new(), format!("http://{address}"));
    (confluence, log)
}

fn site(token: &SecretString) -> Site<'_> {
    Site {
        url: SITE_URL,
        email: EMAIL,
        token,
    }
}

fn space(key: &str) -> Space {
    Space {
        id: key.to_lowercase(),
        key: key.to_owned(),
        name: key.to_owned(),
    }
}

/// The CQL the fake was last sent, decoded.
fn last_cql(log: &Log) -> String {
    let received = log.lock().expect("lock").clone();
    let search = received
        .iter()
        .rev()
        .find(|request| {
            request.path == "/wiki/rest/api/content/search" && request.query.contains("cql=")
        })
        .expect("a search was sent");
    url::form_urlencoded::parse(search.query.as_bytes())
        .find(|(key, _value)| key == "cql")
        .map(|(_key, value)| value.into_owned())
        .expect("the search carries cql")
}

#[tokio::test]
async fn a_token_confluence_accepts_reports_whose_it_is() {
    let (confluence, _log) = fake_confluence_site().await;
    let token = SecretString::from(TOKEN);

    let account = confluence.verify(&site(&token)).await.expect("verified");

    assert_eq!(account.account_id, ACCOUNT_ID);
    assert_eq!(account.display_name, "Alex Navarro");
}

#[tokio::test]
async fn a_token_confluence_refuses_is_unauthorized() {
    let (confluence, _log) = fake_confluence_site().await;
    let token = SecretString::from("not-the-token");

    let error = confluence.verify(&site(&token)).await.expect_err("refused");

    assert!(
        matches!(error, ConfluenceError::Unauthorized(_)),
        "{error:?}"
    );
}

#[tokio::test]
async fn spaces_are_listed_across_pages() {
    let (confluence, _log) = fake_confluence_site().await;
    let token = SecretString::from(TOKEN);

    let listing = confluence.list_spaces(&site(&token)).await.expect("listed");

    let keys: Vec<&str> = listing
        .items
        .iter()
        .map(|space| space.key.as_str())
        .collect();
    assert_eq!(keys, ["ENG", "SECRET", "~5b10"]);
    assert!(!listing.truncated);
}

#[tokio::test]
async fn a_search_is_bounded_to_the_picked_spaces_in_its_cql_and_its_results() {
    let (confluence, log) = fake_confluence_site().await;
    let token = SecretString::from(TOKEN);
    let scope = SpaceScope::Only(vec![space("ENG"), space("~5b10")]);
    let query = ChangeQuery {
        scope: &scope,
        types: &[ContentType::Page, ContentType::Comment],
        since_minutes: 30,
        max_items: 100,
    };

    let changes = confluence
        .changes(&site(&token), &query)
        .await
        .expect("searched");

    let ids: Vec<&str> = changes
        .items
        .iter()
        .map(|change| change.id.as_str())
        .collect();
    assert_eq!(
        ids,
        ["101", "201"],
        "the SECRET page the fake answered is dropped"
    );
    assert!(changes.complete);
    let received = log.lock().expect("lock").clone();
    let first_search = received
        .iter()
        .find(|request| request.path == "/wiki/rest/api/content/search")
        .expect("a search was sent");
    let cql = url::form_urlencoded::parse(first_search.query.as_bytes())
        .find(|(key, _value)| key == "cql")
        .map(|(_key, value)| value.into_owned())
        .expect("cql");
    assert_eq!(
        cql,
        "type in (page,comment) AND lastmodified >= now(\"-30m\") AND space in (\"ENG\",\"~5b10\") \
         ORDER BY lastmodified ASC"
    );
}

#[tokio::test]
async fn every_space_sends_no_space_clause_and_keeps_every_result() {
    let (confluence, log) = fake_confluence_site().await;
    let token = SecretString::from(TOKEN);
    let query = ChangeQuery {
        scope: &SpaceScope::All,
        types: &[ContentType::Page],
        since_minutes: 5,
        max_items: 100,
    };

    let changes = confluence
        .changes(&site(&token), &query)
        .await
        .expect("searched");

    assert_eq!(changes.items.len(), 3);
    assert!(!last_cql(&log).contains("space in"));
}

#[tokio::test]
async fn an_empty_pick_reads_nothing_and_sends_nothing() {
    let (confluence, log) = fake_confluence_site().await;
    let token = SecretString::from(TOKEN);
    let scope = SpaceScope::Only(Vec::new());
    let query = ChangeQuery {
        scope: &scope,
        types: &[ContentType::Page],
        since_minutes: 5,
        max_items: 100,
    };

    let changes = confluence
        .changes(&site(&token), &query)
        .await
        .expect("searched");

    assert!(changes.items.is_empty());
    assert!(changes.complete);
    assert!(log.lock().expect("lock").is_empty());
}

#[tokio::test]
async fn a_search_stops_at_its_limit_and_says_it_is_incomplete() {
    let (confluence, _log) = fake_confluence_site().await;
    let token = SecretString::from(TOKEN);
    let query = ChangeQuery {
        scope: &SpaceScope::All,
        types: &[ContentType::Page, ContentType::Comment],
        since_minutes: 5,
        max_items: 1,
    };

    let changes = confluence
        .changes(&site(&token), &query)
        .await
        .expect("searched");

    assert_eq!(changes.items.len(), 1);
    assert!(!changes.complete);
}

#[tokio::test]
async fn a_result_carries_its_version_author_link_and_page() {
    let (confluence, _log) = fake_confluence_site().await;
    let token = SecretString::from(TOKEN);
    let scope = SpaceScope::Only(vec![space("ENG")]);
    let query = ChangeQuery {
        scope: &scope,
        types: &[ContentType::Page, ContentType::Comment],
        since_minutes: 5,
        max_items: 100,
    };

    let changes = confluence
        .changes(&site(&token), &query)
        .await
        .expect("searched");

    let page = &changes.items[0];
    assert_eq!(page.content_type, ContentType::Page);
    assert_eq!(page.version, 2);
    assert_eq!(
        page.modified_at.map(|time| time.to_rfc3339()),
        Some("2026-11-02T10:02:00+00:00".to_owned())
    );
    assert_eq!(
        page.url.as_deref(),
        Some("https://acme.atlassian.net/wiki/spaces/ENG/pages/101")
    );
    let comment = &changes.items[1];
    assert_eq!(comment.content_type, ContentType::Comment);
    let container = comment.container.as_ref().expect("a comment's page");
    assert_eq!(container.id, "101");
    assert_eq!(container.created_by.as_deref(), Some(ACCOUNT_ID));
}

#[test]
fn a_space_key_with_a_quote_cannot_break_out_of_its_literal() {
    let scope = SpaceScope::Only(vec![space("A\" OR space = \"B")]);
    let query = ChangeQuery {
        scope: &scope,
        types: &[ContentType::Page],
        since_minutes: 5,
        max_items: 1,
    };
    let cql = cql(&query).expect("a query");
    assert!(cql.contains(r#"space in ("A\" OR space = \"B")"#), "{cql}");
}

#[test]
fn a_scope_is_stored_as_star_or_its_spaces() {
    assert_eq!(
        serde_json::to_value(SpaceScope::All).expect("serializes"),
        json!("*")
    );
    let picked = SpaceScope::Only(vec![space("ENG")]);
    let stored = serde_json::to_value(&picked).expect("serializes");
    assert_eq!(
        stored,
        json!([{ "id": "eng", "key": "ENG", "name": "ENG" }])
    );
    assert_eq!(
        serde_json::from_value::<SpaceScope>(stored).expect("parses"),
        picked
    );
    serde_json::from_value::<SpaceScope>(json!("everything")).expect_err("only * means all");
}
