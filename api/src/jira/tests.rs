// Copyright © 2026 Jalapeno Labs

//! [`Jira`] end to end, against a local fake of Jira Cloud's REST API.
//!
//! The fake answers the way Jira does: HTTP basic auth, `401` with an `errorMessages` body
//! for the wrong credentials, `startAt` paging with `isLast` for listings, token paging for
//! a search, and `204` with no body for a change. It records every request it received, so
//! the tests can assert what was actually sent: the exact JQL, the ADF a comment was wrapped
//! into, and that no answer or record ever carries the token.
//!
//! No live Jira account exists here, so these prove Elysium's half of the contract against
//! the shapes Atlassian documents, not against Atlassian.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, Uri};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::json;

use super::*;

/// The account and token the fake accepts. Anything else is answered `401`, as Jira does.
pub(crate) const EMAIL: &str = "alex@example.com";
pub(crate) const TOKEN: &str = "ATATTsecret-token-value";

/// The site a credential names. Requests go to the fake, but every URL Elysium builds for a
/// person still points here.
pub(crate) const SITE_URL: &str = "https://acme.atlassian.net";

/// How many boards the fake has: more than [`PAGE_LIMIT`] pages, so a listing is truncated.
const BOARD_PAGES: u32 = 12;

/// One request the fake received.
#[derive(Debug, Clone)]
pub(crate) struct Received {
    /// Kept for the `{:?}` a failing assertion prints, which is where it earns its place.
    #[expect(dead_code, reason = "read only in a failed assertion's debug output")]
    method: Method,
    pub(crate) path: String,
    query: String,
    authorization: Option<String>,
    body: Value,
}

pub(crate) type Log = Arc<Mutex<Vec<Received>>>;

fn jira_error(status: StatusCode, message: &str) -> Response {
    (
        status,
        axum::Json(json!({ "errorMessages": [message], "errors": {} })),
    )
        .into_response()
}

/// The `Authorization` header Jira expects for these credentials.
fn expected_authorization() -> String {
    format!("Basic {}", BASE64.encode(format!("{EMAIL}:{TOKEN}")))
}

fn project(id: &str, key: &str, name: &str) -> Value {
    json!({ "id": id, "key": key, "name": name })
}

/// One page of boards, all in the same shape Jira's agile API answers.
fn board_page(start_at: u32) -> Value {
    let values: Vec<Value> = (0..PAGE_SIZE)
        .map(|offset| {
            let id = start_at + offset + 1;
            json!({
                "id": id,
                "name": format!("board {id}"),
                "location": { "projectKey": "ELY", "projectName": "Elysium" },
            })
        })
        .collect();
    json!({ "startAt": start_at, "maxResults": PAGE_SIZE, "isLast": false, "values": values })
}

/// One issue, as Jira answers it with every field a search asks for.
fn issue(key: &str, project_key: &str, with_details: bool) -> Value {
    let mut fields = json!({
        "summary": "Bound every search to the allowlist",
        "status": { "name": "In Progress", "statusCategory": { "key": "indeterminate" } },
        "issuetype": { "name": "Task" },
        "priority": { "name": "High" },
        "assignee": { "accountId": "5b10a2844c20165700ede21g", "displayName": "Alex Navarro" },
        "reporter": { "accountId": "5b10a2844c20165700ede21g", "displayName": "Alex Navarro" },
        "labels": ["backend"],
        "created": "2026-09-17T12:00:00.000+0000",
        "updated": "2026-09-17T12:30:00.000+0000",
        "project": { "id": "10002", "key": project_key, "name": "Elysium" },
    });
    if with_details {
        fields["description"] = json!({
            "type": "doc",
            "version": 1,
            "content": [
                { "type": "paragraph", "content": [{ "type": "text", "text": "What it does." }] },
            ],
        });
        fields["comment"] = json!({
            "comments": [{
                "id": "10100",
                "author": { "accountId": "62a1", "displayName": "Sam" },
                "body": { "type": "doc", "version": 1, "content": [
                    { "type": "paragraph", "content": [{ "type": "text", "text": "Looks right" }] },
                ] },
                "created": "2026-09-17T13:00:00.000+0000",
                "updated": "2026-09-17T13:00:00.000+0000",
            }],
        });
    }

    json!({ "id": "10042", "key": key, "fields": fields })
}

#[expect(
    clippy::too_many_lines,
    reason = "one fake of every route the client calls, read as a table of answers"
)]
async fn fake_jira(
    State(log): State<Log>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: String,
) -> Response {
    let authorization = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let path = uri.path().to_owned();
    let query = uri.query().unwrap_or_default().to_owned();
    log.lock().expect("lock").push(Received {
        method: method.clone(),
        path: path.clone(),
        query: query.clone(),
        authorization: authorization.clone(),
        body: serde_json::from_str(&body).unwrap_or(Value::Null),
    });

    if authorization != Some(expected_authorization()) {
        return jira_error(
            StatusCode::UNAUTHORIZED,
            "Client must be authenticated to access this resource.",
        );
    }

    match (method, path.as_str()) {
        (Method::GET, "/rest/api/3/myself") => axum::Json(json!({
            "accountId": "5b10a2844c20165700ede21g",
            "displayName": "Alex Navarro",
            "emailAddress": EMAIL,
        }))
        .into_response(),

        (Method::GET, "/rest/api/3/project/search") => {
            // Two pages, so the listing proves it follows them and stops on isLast.
            if query.contains("startAt=0") {
                let values: Vec<Value> = (0..PAGE_SIZE)
                    .map(|offset| {
                        let id = 20000 + offset;
                        project(&id.to_string(), &format!("P{offset}"), "Filler")
                    })
                    .collect();
                return axum::Json(json!({ "isLast": false, "values": values })).into_response();
            }
            axum::Json(json!({
                "isLast": true,
                "values": [
                    project("10002", "ELY", "Elysium"),
                    project("10001", "OPS", "Operations"),
                ],
            }))
            .into_response()
        }

        (Method::GET, "/rest/agile/1.0/board") => {
            let start_at: u32 = query
                .split('&')
                .find_map(|parameter| parameter.strip_prefix("startAt="))
                .and_then(|value| value.parse().ok())
                .unwrap_or_default();
            if start_at >= BOARD_PAGES * PAGE_SIZE {
                return axum::Json(json!({ "isLast": true, "values": [] })).into_response();
            }
            axum::Json(board_page(start_at)).into_response()
        }

        (Method::POST, "/rest/api/3/search/jql") => {
            let request: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            let jql = request
                .get("jql")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if jql.contains("broken") {
                return jira_error(
                    StatusCode::BAD_REQUEST,
                    "Field 'broken' does not exist or you do not have permission to view it.",
                );
            }
            let has_token = request.get("nextPageToken").is_some();
            axum::Json(json!({
                "issues": [issue("ELY-12", "ELY", false)],
                // The last page carries no token, and Jira then reports isLast.
                "nextPageToken": if has_token { Value::Null } else { json!("CAEaAggD") },
                "isLast": has_token,
            }))
            .into_response()
        }

        (Method::GET, "/rest/api/3/issue/ELY-12") => {
            axum::Json(issue("ELY-12", "ELY", true)).into_response()
        }
        // An issue whose key says one project and whose fields say another, which is what a
        // moved issue looks like.
        (Method::GET, "/rest/api/3/issue/ELY-99") => {
            axum::Json(issue("ELY-99", "SECRET", true)).into_response()
        }
        (Method::GET, "/rest/api/3/issue/ELY-404") => {
            jira_error(StatusCode::NOT_FOUND, "Issue does not exist or you do not have permission to see it.")
        }

        (Method::POST, "/rest/api/3/issue") => (
            StatusCode::CREATED,
            axum::Json(json!({ "id": "10043", "key": "ELY-13", "self": "…" })),
        )
            .into_response(),
        (Method::PUT, "/rest/api/3/issue/ELY-12") => StatusCode::NO_CONTENT.into_response(),

        // A moved issue answers these the same way its new project would, which is what
        // makes the routes' own check against the project Jira reports worth having.
        (
            Method::GET,
            "/rest/api/3/issue/ELY-12/transitions" | "/rest/api/3/issue/ELY-99/transitions",
        ) => axum::Json(json!({
            "transitions": [{
                "id": "31",
                "name": "Done",
                "to": { "name": "Done", "statusCategory": { "key": "done" } },
            }],
        }))
        .into_response(),
        (Method::POST, "/rest/api/3/issue/ELY-12/transitions") => {
            StatusCode::NO_CONTENT.into_response()
        }

        (
            Method::POST,
            "/rest/api/3/issue/ELY-12/comment" | "/rest/api/3/issue/ELY-99/comment",
        ) => (
            StatusCode::CREATED,
            axum::Json(json!({
                "id": "10101",
                "author": { "accountId": "5b10a2844c20165700ede21g", "displayName": "Alex Navarro" },
                "body": serde_json::from_str::<Value>(&body)
                    .ok()
                    .and_then(|sent| sent.get("body").cloned())
                    .unwrap_or(Value::Null),
                "created": "2026-09-17T14:00:00.000+0000",
                "updated": "2026-09-17T14:00:00.000+0000",
            })),
        )
            .into_response(),

        _other => jira_error(StatusCode::NOT_FOUND, "Not found"),
    }
}

/// Starts the fake on a free local port, and a [`Jira`] pointed at it.
pub(crate) async fn fake_jira_site() -> (Jira, Log) {
    let log = Log::default();
    let router = Router::new()
        .fallback(fake_jira)
        .with_state(Arc::clone(&log));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move { axum::serve(listener, router).await });

    let jira = Jira::with_test_origin(reqwest::Client::new(), format!("http://{address}"));
    (jira, log)
}

fn credentials() -> SecretString {
    SecretString::from(TOKEN)
}

fn site(token: &SecretString) -> Site<'_> {
    Site {
        url: SITE_URL,
        email: EMAIL,
        token,
    }
}

/// Everything the fake was sent, for asserting on what left the process.
pub(crate) fn sent(log: &Log) -> Vec<Received> {
    log.lock().expect("lock").clone()
}

#[tokio::test]
async fn a_token_jira_accepts_reports_the_account_it_belongs_to() {
    let (jira, _log) = fake_jira_site().await;
    let token = credentials();

    let account = jira.verify(&site(&token)).await.expect("verified");

    assert_eq!(account.account_id, "5b10a2844c20165700ede21g");
    assert_eq!(account.display_name, "Alex Navarro");
    assert_eq!(account.email.as_deref(), Some(EMAIL));
}

#[tokio::test]
async fn a_token_jira_refuses_says_what_to_check_and_never_repeats_itself() {
    let (jira, log) = fake_jira_site().await;
    let wrong = SecretString::from("ATATTwrong-token");

    let refused = jira.verify(&site(&wrong)).await.unwrap_err();

    let JiraError::Unauthorized(message) = refused else {
        panic!("a refused token is the caller's to fix, not an upstream fault");
    };
    assert!(message.contains("revoked"), "{message}");
    assert!(
        !message.contains("ATATTwrong-token"),
        "an error never carries the token: {message}"
    );
    assert_eq!(sent(&log).len(), 1, "one call, not a retry loop");
}

#[tokio::test]
async fn projects_follow_every_page_and_boards_report_a_listing_cut_short() {
    let (jira, log) = fake_jira_site().await;
    let token = credentials();

    let projects = jira.list_projects(&site(&token)).await.expect("projects");
    assert_eq!(
        projects.items.len(),
        PAGE_SIZE as usize + 2,
        "both pages, and no more once Jira reports the last"
    );
    assert!(!projects.truncated);
    assert_eq!(
        projects.items.last().expect("a project"),
        &Project {
            id: "10001".to_owned(),
            key: "OPS".to_owned(),
            name: "Operations".to_owned(),
        }
    );

    let boards = jira.list_boards(&site(&token)).await.expect("boards");
    assert_eq!(boards.items.len(), (PAGE_LIMIT * PAGE_SIZE) as usize);
    assert!(
        boards.truncated,
        "a site with more boards than the cap says so rather than pretending to be whole"
    );
    assert_eq!(
        boards
            .items
            .first()
            .expect("a board")
            .project_key
            .as_deref(),
        Some("ELY")
    );

    let pages = sent(&log);
    assert_eq!(
        pages
            .iter()
            .filter(|request| request.path == "/rest/agile/1.0/board")
            .count(),
        PAGE_LIMIT as usize,
        "the cap is what stops it, not the fake"
    );
    assert!(
        pages
            .iter()
            .all(|request| request.query.contains("maxResults=50"))
    );
}

#[tokio::test]
async fn a_search_sends_the_jql_it_was_given_and_pages_by_token() {
    let (jira, log) = fake_jira_site().await;
    let token = credentials();
    let jql = r#"(status = Open) AND project IN ("ELY") ORDER BY updated DESC"#;

    let first = jira
        .search(&site(&token), jql, 25, None)
        .await
        .expect("searched");
    assert_eq!(first.issues.len(), 1);
    assert_eq!(first.next_page_token.as_deref(), Some("CAEaAggD"));
    assert!(!first.is_last);

    let second = jira
        .search(&site(&token), jql, 25, first.next_page_token.as_deref())
        .await
        .expect("searched");
    assert_eq!(second.next_page_token, None);
    assert!(second.is_last, "no token means the last page");

    let requests = sent(&log);
    let first_request = &requests[0].body;
    assert_eq!(
        first_request.get("jql").and_then(Value::as_str),
        Some(jql),
        "the bounded JQL is sent verbatim, with no filtering afterwards"
    );
    assert_eq!(first_request.get("maxResults"), Some(&json!(25)));
    assert_eq!(
        first_request.get("nextPageToken"),
        None,
        "the first page sends no token at all, not a null one"
    );
    assert_eq!(
        first_request.get("fields"),
        Some(&json!(SEARCH_FIELDS)),
        "every search names its fields, and project is among them"
    );
    assert_eq!(
        requests[1].body.get("nextPageToken"),
        Some(&json!("CAEaAggD"))
    );
}

#[tokio::test]
async fn jql_jira_will_not_run_is_the_callers_to_fix_and_carries_jiras_words() {
    let (jira, _log) = fake_jira_site().await;
    let token = credentials();

    let refused = jira
        .search(&site(&token), "broken = 1", 50, None)
        .await
        .unwrap_err();

    let JiraError::Invalid(message) = refused else {
        panic!("malformed JQL is a bad request, not a bad gateway");
    };
    assert!(message.contains("does not exist"), "{message}");
}

#[tokio::test]
async fn an_issue_reads_its_fields_its_text_and_where_a_person_opens_it() {
    let (jira, _log) = fake_jira_site().await;
    let token = credentials();

    let issue = jira.issue(&site(&token), "ELY-12").await.expect("issue");

    assert_eq!(issue.key, "ELY-12");
    assert_eq!(issue.project_key, "ELY");
    assert_eq!(issue.summary, "Bound every search to the allowlist");
    assert_eq!(
        issue.status,
        Some(Status {
            name: "In Progress".to_owned(),
            category: "indeterminate".to_owned(),
        })
    );
    assert_eq!(issue.issue_type.as_deref(), Some("Task"));
    assert_eq!(issue.priority.as_deref(), Some("High"));
    assert_eq!(issue.labels, ["backend"]);
    assert_eq!(
        issue.url, "https://acme.atlassian.net/browse/ELY-12",
        "the link is to the credential's own site, never to wherever the call went"
    );
    assert_eq!(
        issue.updated_at.expect("updated").to_rfc3339(),
        "2026-09-17T12:30:00+00:00",
        "Jira writes an offset without a colon, which is not RFC 3339"
    );

    let description = issue.description.expect("a description");
    assert_eq!(description.text, "What it does.");
    assert_eq!(
        description.adf.pointer("/type").and_then(Value::as_str),
        Some("doc"),
        "the document Jira stored comes back untouched beside the rendering"
    );
    assert_eq!(issue.comments.len(), 1);
    assert_eq!(issue.comments[0].body.text, "Looks right");
    assert_eq!(
        issue.comments[0]
            .author
            .as_ref()
            .expect("an author")
            .display_name,
        "Sam"
    );
}

#[tokio::test]
async fn a_search_result_leaves_out_what_only_one_issue_carries() {
    let (jira, _log) = fake_jira_site().await;
    let token = credentials();

    let found = jira
        .search(&site(&token), "project IN (\"ELY\")", 50, None)
        .await
        .expect("searched");

    let issue = found.issues.first().expect("an issue");
    assert_eq!(issue.description, None);
    assert!(issue.comments.is_empty());
    assert_eq!(issue.summary, "Bound every search to the allowlist");
}

#[tokio::test]
async fn the_project_jira_reports_wins_over_the_key_that_was_asked_for() {
    let (jira, _log) = fake_jira_site().await;
    let token = credentials();

    let moved = jira.issue(&site(&token), "ELY-99").await.expect("issue");

    assert_eq!(
        moved.project_key, "SECRET",
        "a moved issue reports where it is now, which is what the allowlist re-checks"
    );
}

#[tokio::test]
async fn an_issue_that_is_not_there_is_not_an_upstream_failure() {
    let (jira, _log) = fake_jira_site().await;
    let token = credentials();

    let missing = jira.issue(&site(&token), "ELY-404").await.unwrap_err();

    assert!(matches!(missing, JiraError::NotFound));
}

#[tokio::test]
async fn writes_wrap_their_text_into_a_document_jira_accepts() {
    let (jira, log) = fake_jira_site().await;
    let token = credentials();

    let key = jira
        .create_issue(
            &site(&token),
            &NewIssue {
                project_key: "ELY".to_owned(),
                issue_type: "Task".to_owned(),
                summary: "Written by Elysium".to_owned(),
                description: Some("First line\n\nSecond paragraph".to_owned()),
                labels: Some(vec!["backend".to_owned()]),
                parent_key: Some("ELY-1".to_owned()),
                ..NewIssue::default()
            },
        )
        .await
        .expect("created");
    assert_eq!(key, "ELY-13");

    let created = &sent(&log)[0].body["fields"];
    assert_eq!(created["project"], json!({ "key": "ELY" }));
    assert_eq!(created["issuetype"], json!({ "name": "Task" }));
    assert_eq!(created["parent"], json!({ "key": "ELY-1" }));
    assert_eq!(created["labels"], json!(["backend"]));
    assert_eq!(
        created["description"],
        adf::from_plain_text("First line\n\nSecond paragraph"),
        "plain text reaches Jira as ADF, since Cloud stores no strings"
    );
    assert_eq!(
        created["description"]["content"]
            .as_array()
            .expect("paragraphs")
            .len(),
        2
    );

    let comment = jira
        .add_comment(&site(&token), "ELY-12", "Looks right")
        .await
        .expect("commented");
    assert_eq!(comment.body.text, "Looks right");
    let requests = sent(&log);
    assert_eq!(
        requests.last().expect("a request").body["body"],
        adf::from_plain_text("Looks right")
    );
}

#[tokio::test]
async fn a_change_sends_only_the_fields_it_was_given() {
    let (jira, log) = fake_jira_site().await;
    let token = credentials();

    jira.update_issue(
        &site(&token),
        "ELY-12",
        &IssueChanges {
            summary: Some("Renamed".to_owned()),
            assignee_account_id: Some(None),
            ..IssueChanges::default()
        },
    )
    .await
    .expect("updated");

    let fields = &sent(&log)[0].body["fields"];
    assert_eq!(fields["summary"], json!("Renamed"));
    assert_eq!(
        fields["assignee"],
        Value::Null,
        "a null assignee is how Jira unassigns an issue"
    );
    assert_eq!(
        fields.as_object().expect("fields").len(),
        2,
        "nothing else is sent, so nothing else is overwritten"
    );
}

#[tokio::test]
async fn transitions_are_listed_and_applied_with_an_optional_comment() {
    let (jira, log) = fake_jira_site().await;
    let token = credentials();

    let transitions = jira
        .transitions(&site(&token), "ELY-12")
        .await
        .expect("transitions");
    assert_eq!(
        transitions,
        vec![Transition {
            id: "31".to_owned(),
            name: "Done".to_owned(),
            to: Some(Status {
                name: "Done".to_owned(),
                category: "done".to_owned(),
            }),
        }]
    );

    jira.apply_transition(&site(&token), "ELY-12", "31", Some("Shipping it"))
        .await
        .expect("transitioned");

    let requests = sent(&log);
    let applied = &requests.last().expect("a request").body;
    assert_eq!(applied["transition"], json!({ "id": "31" }));
    assert_eq!(
        applied["update"]["comment"][0]["add"]["body"],
        adf::from_plain_text("Shipping it")
    );
}

#[tokio::test]
async fn nothing_elysium_returns_or_sends_carries_the_token_in_the_clear() {
    let (jira, log) = fake_jira_site().await;
    let token = credentials();

    let account = jira.verify(&site(&token)).await.expect("verified");
    let issue = jira.issue(&site(&token), "ELY-12").await.expect("issue");
    let found = jira
        .search(&site(&token), "project IN (\"ELY\")", 50, None)
        .await
        .expect("searched");

    for answer in [
        serde_json::to_string(&account).expect("serializes"),
        serde_json::to_string(&issue).expect("serializes"),
        serde_json::to_string(&found).expect("serializes"),
        format!("{issue:?}"),
        format!("{:?}", site(&token)),
    ] {
        assert!(
            !answer.contains(TOKEN),
            "a response or a debug line carried the token: {answer}"
        );
    }

    // The token leaves only where it has to: the Authorization header of each request.
    for request in sent(&log) {
        assert!(!request.body.to_string().contains(TOKEN), "{request:?}");
        assert!(!request.query.contains(TOKEN), "{request:?}");
        assert_eq!(
            request.authorization.as_deref(),
            Some(expected_authorization().as_str())
        );
    }
}

#[tokio::test]
async fn a_site_that_cannot_be_reached_is_an_upstream_failure() {
    // Nothing listens here: port 1 is reserved and never bound.
    let jira = Jira::with_test_origin(reqwest::Client::new(), "http://127.0.0.1:1".to_owned());
    let token = credentials();

    let refused = jira.verify(&site(&token)).await.unwrap_err();

    let JiraError::Refused(message) = refused else {
        panic!("an unreachable site is a bad gateway, not a bad request");
    };
    assert!(!message.contains(TOKEN), "{message}");
}

#[test]
fn only_a_jira_cloud_origin_is_accepted_and_it_is_normalized() {
    for (given, expected) in [
        ("https://acme.atlassian.net", "https://acme.atlassian.net"),
        ("https://acme.atlassian.net/", "https://acme.atlassian.net"),
        (
            "  https://ACME.atlassian.net/  ",
            "https://acme.atlassian.net",
        ),
        (
            "https://acme.jira-dev.atlassian.net",
            "https://acme.jira-dev.atlassian.net",
        ),
    ] {
        assert_eq!(normalize_site_url(given).expect(given), expected);
    }

    for refused in [
        "http://acme.atlassian.net",
        "https://atlassian.net",
        "https://acme.atlassian.net.evil.com",
        "https://jira.example.com",
        "https://user:pass@acme.atlassian.net",
        "https://acme.atlassian.net:8443",
        "https://acme.atlassian.net/jira",
        "https://acme.atlassian.net/?x=1",
        "https://acme.atlassian.net/#fragment",
        "acme.atlassian.net",
        "",
        // A host the site_url column's own constraint would refuse, answered here as a bad
        // request rather than as a failed insert.
        "https://-acme.atlassian.net",
        "https://acme..atlassian.net",
        "https://acme_test.atlassian.net",
    ] {
        normalize_site_url(refused).expect_err(refused);
    }

    let long = format!("https://{}.atlassian.net", "a".repeat(300));
    normalize_site_url(&long).expect_err("a site URL longer than any real one");
}

#[test]
fn an_issue_key_names_its_project_and_nothing_else() {
    assert_eq!(project_of_issue_key("ELY-12"), Some("ELY"));
    assert_eq!(project_of_issue_key("MY_PROJECT2-1"), Some("MY_PROJECT2"));

    for not_a_key in [
        "ELY",
        "ELY-",
        "-12",
        "ELY-12-3",
        "ELY-1a",
        "E-1",
        "../secret-1",
        "ELY 12",
        "",
    ] {
        assert_eq!(project_of_issue_key(not_a_key), None, "{not_a_key}");
    }
}

#[test]
fn a_timestamp_in_an_unknown_shape_is_missing_rather_than_fatal() {
    assert_eq!(
        parse_timestamp("2026-09-17T12:30:00.000+0000")
            .expect("Jira's own shape")
            .to_rfc3339(),
        "2026-09-17T12:30:00+00:00"
    );
    assert_eq!(
        parse_timestamp("2026-09-17T12:30:00+00:00")
            .expect("RFC 3339")
            .to_rfc3339(),
        "2026-09-17T12:30:00+00:00"
    );
    assert_eq!(
        parse_timestamp("2026-09-17T13:30:00.000+0100")
            .expect("under an offset")
            .to_rfc3339(),
        "2026-09-17T12:30:00+00:00"
    );

    assert_eq!(parse_timestamp("yesterday"), None);
    assert_eq!(parse_timestamp(""), None);
}
