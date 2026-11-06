// Copyright © 2026 Jalapeno Labs

//! [`Youtrack`] end to end, against a local fake of YouTrack Cloud's REST API.
//!
//! The fake answers the way YouTrack does: a bearer permanent token, `401` with an
//! `error_description` for the wrong one, `$skip`/`$top` paging, millisecond timestamps, the
//! assignee and the state inside `customFields`, and a project's state field with its resolved
//! values in the project's settings. It opens issues, takes comments, and moves an issue's state
//! the way YouTrack does, and its issues and comments can be changed between calls, so the
//! reader's and the links' tests can play out what happens on a site over time. It records every
//! request.
//!
//! No live YouTrack account is used here, so these prove Elysium's half of the contract against
//! the shapes JetBrains documents and the ones measured on jalapenolabs.youtrack.cloud.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::get;

use super::*;

/// The token the fake accepts. Anything else is answered `401`, as YouTrack does.
pub(crate) const TOKEN: &str = "perm:YWxleA==.ZWx5c2l1bQ==.secret";

/// The site an account names. Requests go to the fake, but every URL Elysium builds for a
/// person still points here.
pub(crate) const SITE_URL: &str = "https://acme.youtrack.cloud";

/// The token's user.
pub(crate) const ME: &str = "alex";

/// The state field every fake project has, named as the default template names it, and its
/// values: three to do, then three resolved, `Fixed` among them.
pub(crate) const STATE_FIELD: &str = "State";
const STATES: [(&str, bool); 6] = [
    ("Submitted", false),
    ("Open", false),
    ("In Progress", false),
    ("Duplicate", true),
    ("Fixed", true),
    ("Won't fix", true),
];

/// One issue the fake holds.
#[derive(Debug, Clone)]
pub(crate) struct FakeIssue {
    pub(crate) id: String,
    pub(crate) key: String,
    pub(crate) project: String,
    pub(crate) summary: String,
    pub(crate) description: String,
    pub(crate) created: i64,
    pub(crate) updated: i64,
    pub(crate) reporter: String,
    pub(crate) assignee: Option<String>,
    /// A value of [`STATE_FIELD`]; the issue is resolved while it is a resolved one.
    pub(crate) state: String,
}

impl FakeIssue {
    fn is_resolved(&self) -> bool {
        STATES
            .iter()
            .any(|(name, is_resolved)| *is_resolved && *name == self.state)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FakeComment {
    pub(crate) issue_id: String,
    pub(crate) id: String,
    pub(crate) text: String,
    pub(crate) created: i64,
    pub(crate) author: String,
}

impl FakeComment {
    pub(crate) fn new(issue_id: &str, id: &str, text: &str, created: i64, author: &str) -> Self {
        Self {
            issue_id: issue_id.to_owned(),
            id: id.to_owned(),
            text: text.to_owned(),
            created,
            author: author.to_owned(),
        }
    }
}

/// What the fake holds and what it was asked, shared with the test that started it.
#[derive(Debug, Default)]
pub(crate) struct FakeSite {
    pub(crate) issues: Vec<FakeIssue>,
    pub(crate) comments: Vec<FakeComment>,
    /// Every request's method, path, and query, in order.
    pub(crate) requests: Vec<String>,
    /// The body of every write, in order, with its path.
    pub(crate) writes: Vec<(String, Value)>,
    /// When set, a search naming issues by id is refused, as a site may refuse a query.
    pub(crate) refuses_id_searches: bool,
}

pub(crate) type Shared = Arc<Mutex<FakeSite>>;

/// An issue `bea` reported and assigned to [`ME`], open, created and last updated at `updated`,
/// in the project its key names. The fake's projects are `JL`, `WAY`, and an archived `OLD`.
pub(crate) fn sample_issue(id: &str, key: &str, updated: i64) -> FakeIssue {
    FakeIssue {
        id: id.to_owned(),
        key: key.to_owned(),
        project: key.split('-').next().unwrap_or("JL").to_owned(),
        summary: "Ship the connector hub".to_owned(),
        description: "Every service in one list.".to_owned(),
        created: updated,
        updated,
        reporter: "bea".to_owned(),
        assignee: Some(ME.to_owned()),
        state: "Open".to_owned(),
    }
}

fn project_id(short_name: &str) -> &'static str {
    match short_name {
        "WAY" => "0-2",
        _ => "0-1",
    }
}

fn project_short_name(id: &str) -> Option<&'static str> {
    match id {
        "0-1" => Some("JL"),
        "0-2" => Some("WAY"),
        _ => None,
    }
}

fn youtrack_error(status: axum::http::StatusCode, description: &str) -> Response {
    (
        status,
        axum::Json(json!({ "error": "Unauthorized", "error_description": description })),
    )
        .into_response()
}

fn not_found() -> Response {
    (
        axum::http::StatusCode::NOT_FOUND,
        axum::Json(json!({ "error": "Not Found", "error_description": "Entity not found" })),
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

fn record(shared: &Shared, method: &str, path: &str, query: &[(String, String)]) {
    let rendered: Vec<String> = query
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    let line = format!("{method} {path}?{}", rendered.join("&"));
    shared.lock().expect("lock").requests.push(line);
}

/// An issue as YouTrack answers one.
fn issue_json(issue: &FakeIssue) -> Value {
    let resolved = issue.is_resolved().then_some(issue.updated);
    json!({
        "id": issue.id,
        "idReadable": issue.key,
        "summary": issue.summary,
        "description": issue.description,
        "created": issue.created,
        "updated": issue.updated,
        "resolved": resolved,
        "reporter": user(&issue.reporter),
        "project": { "id": project_id(&issue.project), "shortName": issue.project },
        "customFields": [
            { "$type": "SingleEnumIssueCustomField", "name": "Priority", "value": { "name": "Major" } },
            {
                "$type": "StateIssueCustomField",
                "name": STATE_FIELD,
                "value": { "name": issue.state },
            },
            {
                "$type": "SingleUserIssueCustomField",
                "name": "Assignee",
                "value": issue.assignee.as_deref().map(user),
            },
        ],
    })
}

async fn me(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<Vec<(String, String)>>,
) -> Response {
    record(&shared, "GET", "/api/users/me", &query);
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
    record(&shared, "GET", "/api/admin/projects", &query);
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

/// A project's custom fields: a priority and the state field, with an archived value left in
/// to prove it is skipped.
async fn project_fields(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Query(query): Query<Vec<(String, String)>>,
) -> Response {
    record(
        &shared,
        "GET",
        &format!("/api/admin/projects/{project}/customFields"),
        &query,
    );
    if !authorized(&headers) {
        return youtrack_error(axum::http::StatusCode::UNAUTHORIZED, "Invalid token");
    }
    if project_short_name(&project).is_none() {
        return not_found();
    }
    let mut values: Vec<Value> = STATES
        .iter()
        .map(|(name, is_resolved)| json!({ "name": name, "isResolved": is_resolved }))
        .collect();
    values.push(json!({ "name": "Done", "isResolved": true, "archived": true }));
    axum::Json(json!([
        { "$type": "EnumProjectCustomField", "field": { "name": "Priority" }, "bundle": { "values": [] } },
        { "$type": "StateProjectCustomField", "field": { "name": STATE_FIELD }, "bundle": { "values": values } },
    ]))
    .into_response()
}

async fn issues(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<Vec<(String, String)>>,
) -> Response {
    record(&shared, "GET", "/api/issues", &query);
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
    // `for: me` asks for the token's user's issues, and `#Unresolved` for unresolved ones; what is
    // left names projects or issues.
    let asked = parameter("query");
    let for_me = asked.contains("for: me");
    let unresolved_only = asked.contains("#Unresolved");
    let text = asked
        .replace(" for: me", "")
        .replace("for: me", "")
        .replace(" #Unresolved", "")
        .replace("#Unresolved", "")
        .trim()
        .to_owned();
    let skip: usize = parameter("$skip").parse().unwrap_or(0);
    let top: usize = parameter("$top").parse().unwrap_or(42);
    let listed = |prefix: &str| -> Option<Vec<String>> {
        text.strip_prefix(prefix)
            .and_then(|rest| rest.split(" sort by:").next())
            .map(|list| list.split(", ").map(str::to_owned).collect())
    };
    // `project: JL, WAY sort by: updated desc` names the projects asked for, and
    // `issue id: JL-1, JL-2 sort by: updated desc` the issues.
    let projects = listed("project: ");
    let keys = listed("issue id: ");
    if keys.is_some() && shared.lock().expect("lock").refuses_id_searches {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(json!({ "error": "bad_request", "error_description": "Cannot parse" })),
        )
            .into_response();
    }

    let mut held = shared.lock().expect("lock").issues.clone();
    held.sort_by_key(|issue| std::cmp::Reverse(issue.updated));
    let answered: Vec<Value> = held
        .iter()
        .filter(|issue| {
            projects
                .as_ref()
                .is_none_or(|names| names.contains(&issue.project))
        })
        .filter(|issue| keys.as_ref().is_none_or(|keys| keys.contains(&issue.key)))
        .filter(|issue| !for_me || issue.assignee.as_deref() == Some(ME))
        .filter(|issue| !unresolved_only || !issue.is_resolved())
        .skip(skip)
        .take(top)
        .map(issue_json)
        .collect();
    axum::Json(Value::Array(answered)).into_response()
}

/// One issue, by its database id or its key.
async fn issue(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Path(reference): Path<String>,
    Query(query): Query<Vec<(String, String)>>,
) -> Response {
    record(&shared, "GET", &format!("/api/issues/{reference}"), &query);
    if !authorized(&headers) {
        return youtrack_error(axum::http::StatusCode::UNAUTHORIZED, "Invalid token");
    }
    let site = shared.lock().expect("lock");
    match site
        .issues
        .iter()
        .find(|issue| issue.id == reference || issue.key == reference)
    {
        Some(found) => axum::Json(issue_json(found)).into_response(),
        None => not_found(),
    }
}

/// Opens an issue as [`ME`], numbered after the project's last.
async fn create_issue(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<Vec<(String, String)>>,
    axum::Json(body): axum::Json<Value>,
) -> Response {
    record(&shared, "POST", "/api/issues", &query);
    if !authorized(&headers) {
        return youtrack_error(axum::http::StatusCode::UNAUTHORIZED, "Invalid token");
    }
    let mut site = shared.lock().expect("lock");
    site.writes.push(("/api/issues".to_owned(), body.clone()));
    let Some(project) = body["project"]["id"].as_str().and_then(project_short_name) else {
        return not_found();
    };
    let number = site
        .issues
        .iter()
        .filter(|issue| issue.project == project)
        .count()
        + 1;
    let created = FakeIssue {
        id: format!("2-{}", 100 + site.issues.len()),
        key: format!("{project}-{number}"),
        project: project.to_owned(),
        summary: body["summary"].as_str().unwrap_or_default().to_owned(),
        description: body["description"].as_str().unwrap_or_default().to_owned(),
        created: 9_000_000,
        updated: 9_000_000,
        reporter: ME.to_owned(),
        assignee: None,
        state: "Submitted".to_owned(),
    };
    let answer = issue_json(&created);
    site.issues.push(created);
    axum::Json(answer).into_response()
}

/// Moves an issue's state, the one field Elysium writes.
async fn update_issue(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Path(reference): Path<String>,
    Query(query): Query<Vec<(String, String)>>,
    axum::Json(body): axum::Json<Value>,
) -> Response {
    record(&shared, "POST", &format!("/api/issues/{reference}"), &query);
    if !authorized(&headers) {
        return youtrack_error(axum::http::StatusCode::UNAUTHORIZED, "Invalid token");
    }
    let mut site = shared.lock().expect("lock");
    site.writes
        .push((format!("/api/issues/{reference}"), body.clone()));
    let field = &body["customFields"][0];
    let Some(state) = field["value"]["name"].as_str() else {
        return youtrack_error(axum::http::StatusCode::BAD_REQUEST, "No value");
    };
    if field["name"] != json!(STATE_FIELD) || !STATES.iter().any(|(name, _)| *name == state) {
        return youtrack_error(axum::http::StatusCode::BAD_REQUEST, "Unknown state");
    }
    let Some(found) = site.issues.iter_mut().find(|issue| issue.id == reference) else {
        return not_found();
    };
    state.clone_into(&mut found.state);
    found.updated += 1;
    axum::Json(json!({ "id": reference })).into_response()
}

async fn comments(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Path(issue_id): Path<String>,
    Query(query): Query<Vec<(String, String)>>,
) -> Response {
    record(
        &shared,
        "GET",
        &format!("/api/issues/{issue_id}/comments"),
        &query,
    );
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
                "author": user(&comment.author),
            })
        })
        .collect();
    axum::Json(Value::Array(answered)).into_response()
}

async fn add_comment(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Path(issue_id): Path<String>,
    Query(query): Query<Vec<(String, String)>>,
    axum::Json(body): axum::Json<Value>,
) -> Response {
    record(
        &shared,
        "POST",
        &format!("/api/issues/{issue_id}/comments"),
        &query,
    );
    if !authorized(&headers) {
        return youtrack_error(axum::http::StatusCode::UNAUTHORIZED, "Invalid token");
    }
    let mut site = shared.lock().expect("lock");
    site.writes
        .push((format!("/api/issues/{issue_id}/comments"), body.clone()));
    if !site.issues.iter().any(|issue| issue.id == issue_id) {
        return not_found();
    }
    let id = format!("4-{}", 100 + site.comments.len());
    let text = body["text"].as_str().unwrap_or_default();
    site.comments
        .push(FakeComment::new(&issue_id, &id, text, 9_000_000, ME));
    axum::Json(json!({ "id": id })).into_response()
}

/// Starts the fake on a free local port, and a [`Youtrack`] pointed at it.
pub(crate) async fn fake_youtrack_site(site: FakeSite) -> (Youtrack, Shared) {
    let shared: Shared = Arc::new(Mutex::new(site));
    let router = Router::new()
        .route("/api/users/me", get(me))
        .route("/api/admin/projects", get(projects))
        .route(
            "/api/admin/projects/{project_id}/customFields",
            get(project_fields),
        )
        .route("/api/issues", get(issues).post(create_issue))
        .route("/api/issues/{issue_id}", get(issue).post(update_issue))
        .route(
            "/api/issues/{issue_id}/comments",
            get(comments).post(add_comment),
        )
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
        comments: vec![FakeComment::new(
            "2-1",
            "4-7",
            "Looks good to me",
            5_000,
            "bea",
        )],
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
