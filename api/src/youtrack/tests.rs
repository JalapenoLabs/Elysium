// Copyright © 2026 Jalapeno Labs

//! [`Youtrack`] end to end, against a local fake of YouTrack Cloud's REST API.
//!
//! The fake answers the way YouTrack does: a bearer permanent token, `401` with an
//! `error_description` for the wrong one, `$skip`/`$top` paging, millisecond timestamps, and
//! the assignee inside `customFields`. Its issues and comments can be changed between calls, so
//! the reader's tests can play out what happens on a site over time. It records every request.
//!
//! No live YouTrack account is used here, so these prove Elysium's half of the contract against
//! the shapes JetBrains documents and the ones measured on jalapenolabs.youtrack.cloud.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::{Value, json};

use super::*;

/// The token the fake accepts. Anything else is answered `401`, as YouTrack does.
pub(crate) const TOKEN: &str = "perm:YWxleA==.ZWx5c2l1bQ==.secret";

/// The site an account names. Requests go to the fake, but every URL Elysium builds for a
/// person still points here.
pub(crate) const SITE_URL: &str = "https://acme.youtrack.cloud";

/// The token's user.
pub(crate) const ME: &str = "alex";

/// One issue the fake holds.
#[derive(Debug, Clone)]
pub(crate) struct FakeIssue {
    pub(crate) id: &'static str,
    pub(crate) key: &'static str,
    pub(crate) project: &'static str,
    pub(crate) summary: &'static str,
    pub(crate) created: i64,
    pub(crate) updated: i64,
    pub(crate) reporter: &'static str,
    pub(crate) assignee: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub(crate) struct FakeComment {
    pub(crate) issue_id: &'static str,
    pub(crate) id: &'static str,
    pub(crate) text: &'static str,
    pub(crate) created: i64,
    pub(crate) author: &'static str,
}

/// What the fake holds and what it was asked, shared with the test that started it.
#[derive(Debug, Default)]
pub(crate) struct FakeSite {
    pub(crate) issues: Vec<FakeIssue>,
    pub(crate) comments: Vec<FakeComment>,
    /// Every request's path and query, in order.
    pub(crate) requests: Vec<String>,
}

pub(crate) type Shared = Arc<Mutex<FakeSite>>;

/// An issue `bea` reported and assigned to [`ME`], created and last updated at `updated`, in
/// the project its key names. The fake's projects are `JL`, `WAY`, and an archived `OLD`.
pub(crate) fn sample_issue(id: &'static str, key: &'static str, updated: i64) -> FakeIssue {
    FakeIssue {
        id,
        key,
        project: key.split('-').next().unwrap_or("JL"),
        summary: "Ship the connector hub",
        created: updated,
        updated,
        reporter: "bea",
        assignee: Some(ME),
    }
}

fn project_id(short_name: &str) -> &'static str {
    match short_name {
        "WAY" => "0-2",
        _ => "0-1",
    }
}

fn youtrack_error(status: axum::http::StatusCode, description: &str) -> Response {
    (
        status,
        axum::Json(json!({ "error": "Unauthorized", "error_description": description })),
    )
        .into_response()
}

fn authorized(headers: &HeaderMap) -> bool {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == format!("Bearer {TOKEN}"))
}

fn user(login: &str) -> Value {
    json!({ "login": login, "fullName": format!("{login} (full name)") })
}

fn record(shared: &Shared, path: &str, query: &[(String, String)]) {
    let rendered: Vec<String> = query
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    shared
        .lock()
        .expect("lock")
        .requests
        .push(format!("{path}?{}", rendered.join("&")));
}

async fn me(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<Vec<(String, String)>>,
) -> Response {
    record(&shared, "/api/users/me", &query);
    if !authorized(&headers) {
        return youtrack_error(axum::http::StatusCode::UNAUTHORIZED, "Invalid token");
    }
    axum::Json(user(ME)).into_response()
}

async fn projects(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<Vec<(String, String)>>,
) -> Response {
    record(&shared, "/api/admin/projects", &query);
    if !authorized(&headers) {
        return youtrack_error(axum::http::StatusCode::UNAUTHORIZED, "Invalid token");
    }
    axum::Json(json!([
        { "id": "0-1", "shortName": "JL", "name": "Jalapeno Labs", "archived": false },
        { "id": "0-2", "shortName": "WAY", "name": "Waystead", "archived": false },
        { "id": "0-3", "shortName": "OLD", "name": "Retired", "archived": true },
    ]))
    .into_response()
}

async fn issues(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<Vec<(String, String)>>,
) -> Response {
    record(&shared, "/api/issues", &query);
    if !authorized(&headers) {
        return youtrack_error(axum::http::StatusCode::UNAUTHORIZED, "Invalid token");
    }
    let parameter = |name: &str| {
        query
            .iter()
            .find(|(key, _value)| key == name)
            .map(|(_key, value)| value.clone())
            .unwrap_or_default()
    };
    let text = parameter("query");
    let skip: usize = parameter("$skip").parse().unwrap_or(0);
    let top: usize = parameter("$top").parse().unwrap_or(42);
    // `project: JL, WAY sort by: updated desc` names the projects asked for, if any.
    let named: Option<Vec<String>> = text
        .strip_prefix("project: ")
        .and_then(|rest| rest.split(" sort by:").next())
        .map(|list| list.split(", ").map(str::to_owned).collect());

    let mut held = shared.lock().expect("lock").issues.clone();
    held.sort_by_key(|issue| std::cmp::Reverse(issue.updated));
    let answered: Vec<Value> = held
        .into_iter()
        .filter(|issue| {
            named
                .as_ref()
                .is_none_or(|names| names.iter().any(|name| name == issue.project))
        })
        .skip(skip)
        .take(top)
        .map(|issue| {
            json!({
                "id": issue.id,
                "idReadable": issue.key,
                "summary": issue.summary,
                "description": "Every service in one list.",
                "created": issue.created,
                "updated": issue.updated,
                "resolved": null,
                "reporter": user(issue.reporter),
                "project": { "id": project_id(issue.project), "shortName": issue.project },
                "customFields": [
                    { "name": "Priority", "value": { "name": "Normal" } },
                    { "name": "Assignee", "value": issue.assignee.map(user) },
                ],
            })
        })
        .collect();
    axum::Json(Value::Array(answered)).into_response()
}

async fn comments(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Path(issue_id): Path<String>,
    Query(query): Query<Vec<(String, String)>>,
) -> Response {
    record(&shared, &format!("/api/issues/{issue_id}/comments"), &query);
    if !authorized(&headers) {
        return youtrack_error(axum::http::StatusCode::UNAUTHORIZED, "Invalid token");
    }
    let held = shared.lock().expect("lock").comments.clone();
    let answered: Vec<Value> = held
        .into_iter()
        .filter(|comment| comment.issue_id == issue_id)
        .map(|comment| {
            json!({
                "id": comment.id,
                "text": comment.text,
                "created": comment.created,
                "deleted": false,
                "author": user(comment.author),
            })
        })
        .collect();
    axum::Json(Value::Array(answered)).into_response()
}

/// Starts the fake on a free local port, and a [`Youtrack`] pointed at it.
pub(crate) async fn fake_youtrack_site(site: FakeSite) -> (Youtrack, Shared) {
    let shared: Shared = Arc::new(Mutex::new(site));
    let router = Router::new()
        .route("/api/users/me", get(me))
        .route("/api/admin/projects", get(projects))
        .route("/api/issues", get(issues))
        .route("/api/issues/{issue_id}/comments", get(comments))
        .with_state(Arc::clone(&shared));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move { axum::serve(listener, router).await });

    let youtrack = Youtrack::with_test_origin(reqwest::Client::new(), format!("http://{address}"));
    (youtrack, shared)
}

fn site(token: &SecretString) -> Site<'_> {
    Site {
        url: SITE_URL,
        token,
    }
}

#[test]
fn a_site_is_a_youtrack_cloud_origin_and_nothing_more() {
    assert_eq!(
        normalize_site_url(" https://Acme.youtrack.cloud/ "),
        Ok("https://acme.youtrack.cloud".to_owned())
    );
    for refused in [
        "http://acme.youtrack.cloud",
        "https://acme.youtrack.cloud/issues",
        "https://acme.youtrack.cloud:8443",
        "https://alex:secret@acme.youtrack.cloud",
        "https://acme.myjetbrains.com",
        "https://youtrack.cloud",
        "https://evil.example.com/.youtrack.cloud",
        "https://a.b.youtrack.cloud",
        "https://-acme.youtrack.cloud",
        "acme",
    ] {
        assert!(
            normalize_site_url(refused).is_err(),
            "{refused} was accepted"
        );
    }
}

#[test]
fn only_a_database_id_reaches_an_issue_path() {
    assert!(is_database_id("2-41"));
    for refused in ["JL-12", "2-", "-41", "2-41/../users", "2-41?x=1", ""] {
        assert!(!is_database_id(refused), "{refused} was accepted");
    }
}

#[tokio::test]
async fn a_token_youtrack_accepts_reports_its_user() {
    let (youtrack, _shared) = fake_youtrack_site(FakeSite::default()).await;
    let token = SecretString::from(TOKEN);

    let user = youtrack.whoami(&site(&token)).await.expect("signed in");

    assert_eq!(user.login, ME);
    assert_eq!(user.full_name, "alex (full name)");
}

#[tokio::test]
async fn a_token_youtrack_refuses_says_what_to_check() {
    let (youtrack, _shared) = fake_youtrack_site(FakeSite::default()).await;
    let wrong = SecretString::from("perm:wrong");

    let refused = youtrack.whoami(&site(&wrong)).await.unwrap_err();

    assert!(matches!(refused, YoutrackError::Unauthorized(_)));
    assert!(!refused.to_string().contains("perm:"));
}

#[tokio::test]
async fn archived_projects_are_left_out() {
    let (youtrack, _shared) = fake_youtrack_site(FakeSite::default()).await;
    let token = SecretString::from(TOKEN);

    let projects = youtrack.list_projects(&site(&token)).await.expect("listed");

    let names: Vec<&str> = projects
        .items
        .iter()
        .map(|project| project.short_name.as_str())
        .collect();
    assert_eq!(names, ["JL", "WAY"]);
    assert!(!projects.truncated);
}

#[tokio::test]
async fn a_search_reads_the_assignee_from_its_custom_field() {
    let site_data = FakeSite {
        issues: vec![
            sample_issue("2-1", "JL-1", 1_000),
            FakeIssue {
                assignee: None,
                ..sample_issue("2-2", "WAY-1", 2_000)
            },
        ],
        ..FakeSite::default()
    };
    let (youtrack, shared) = fake_youtrack_site(site_data).await;
    let token = SecretString::from(TOKEN);

    let found = youtrack
        .search(&site(&token), "project: JL, WAY", 0, 10)
        .await
        .expect("searched");

    let keys: Vec<&str> = found.iter().map(|issue| issue.key.as_str()).collect();
    assert_eq!(keys, ["WAY-1", "JL-1"], "most recently updated first");
    assert_eq!(found[0].assignee, None);
    assert_eq!(
        found[1].assignee.as_ref().map(|user| user.login.as_str()),
        Some(ME)
    );
    assert_eq!(found[1].project_id, "0-1");
    let sent = shared.lock().expect("lock").requests.clone();
    assert!(
        sent.iter()
            .any(|request| request.contains("query=project: JL, WAY sort by: updated desc")),
        "{sent:?}"
    );
}

#[tokio::test]
async fn comments_come_with_their_authors() {
    let site_data = FakeSite {
        comments: vec![FakeComment {
            issue_id: "2-1",
            id: "4-7",
            text: "Looks good to me",
            created: 5_000,
            author: "bea",
        }],
        ..FakeSite::default()
    };
    let (youtrack, _shared) = fake_youtrack_site(site_data).await;
    let token = SecretString::from(TOKEN);

    let comments = youtrack
        .comments(&site(&token), "2-1", 10)
        .await
        .expect("read");

    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text, "Looks good to me");
    assert_eq!(
        comments[0].author.as_ref().map(|user| user.login.as_str()),
        Some("bea")
    );
}
