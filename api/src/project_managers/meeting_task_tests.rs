// Copyright © 2026 Jalapeno Labs

//! #243: a meeting's tasks, from segmenting to action items. Segmenting a meeting marks each task
//! it hears; a project's reaches its Project Manager, whose turn makes it into its changeset under
//! the project's level of autonomy, and everyone else's becomes an item at once. Done reviewing
//! reads the meeting again and sends the Project Manager only the task that changed. Driven against
//! Postgres with a scripted segmenting answer and a scripted Project Manager turn, whose tool calls
//! go through the agent-tools endpoint as a real turn's do, once at each level of the dial.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::{TimeDelta, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt as _;
use uuid::Uuid;

use super::dispatcher::{self, DispatchContext};
use super::runner::{ManagerRunner, ManagerTurn, Script};
use crate::action_items::changesets::LinkReads;
use crate::database::schema::{action_items, meeting_task_items, project_manager_wakes};
use crate::elysia::runner::ElysiaOutcome;
use crate::meetings::correct::CorrectedMeeting;
use crate::meetings::segment::{SegmentMeeting, TranscriptToSegment};
use crate::meetings::source::TranscriptLine;
use crate::models::action_item::{self, ActionItemState};
use crate::models::changeset::{self, ChangesetDecision, ChangesetState};
use crate::models::meeting::tests::fetched_with;
use crate::models::meeting::{self, Meeting};
use crate::models::meeting_capture::MeetingCapture;
use crate::models::meeting_mark::{self, MarkKind};
use crate::models::project::{self, NewProject};
use crate::models::project_autonomy::{self, Autonomy};
use crate::models::project_manager::WakeKind;
use crate::models::timeline_record;
use crate::realtime::EventBus;
use crate::routes;
use crate::runner::McpServer;
use crate::state::AppState;
use crate::test_support::{
    TEST_PERSON_ID, app_state, fetched_agent_configuration, migrated_database, signed_in_cookie,
    test_member,
};
use crate::tools::grants::Grant;
use crate::tools::{CallScope, ProjectManagerScope};
use crate::workflows::{Done, Step, StepContext};

/// A coworker on Elysium.
const GRACE_ID: Uuid = uuid::uuid!("0199a3c4-0000-7000-8000-0000000097ac");

/// The meeting, one line a minute apart so each is its own line of the prompt.
fn transcript() -> Vec<TranscriptLine> {
    let said = [
        ("Ada", "Is my mic on?"),
        ("Grace", "I'll fix the login bug on the launch page."),
        ("Tester", "I'll send Ada the deck."),
        ("Grace", "Sharath will book the room."),
        ("Ada", "Someone should ask legal."),
        ("Grace", "I'll write the release notes."),
        ("Grace", "We still have to pick the launch date."),
        ("Ada", "Thanks everyone."),
    ];
    said.iter()
        .zip(0_i64..)
        .map(|((speaker, text), minute)| TranscriptLine {
            start_ms: minute * 60_000,
            end_ms: minute * 60_000 + 3_000,
            speaker: (*speaker).to_owned(),
            text: (*text).to_owned(),
        })
        .collect()
}

fn task(first: usize, last: usize, title: &str, owed_by: &str, project: Option<Uuid>) -> Value {
    json!({
        "firstLine": first,
        "lastLine": last,
        "title": title,
        "owedBy": owed_by,
        "projectId": project.map(|id| id.to_string()).unwrap_or_default(),
    })
}

/// Segmenting's answer: the login bug and the launch date are the launch's, in its excerpt; the
/// rest are on no project. `release_notes` and `launch_date` are what a second reading changes.
fn answer(launch: Uuid, release_notes: (usize, usize), launch_date_owed_by: &str) -> String {
    let grace = GRACE_ID.to_string();
    let tester = TEST_PERSON_ID.to_string();
    json!({
        "excerpts": [{
            "firstLine": 2,
            "lastLine": 7,
            "title": "The launch",
            "summary": "Grace fixes the login bug; the date is still open.",
            "projects": [{ "projectId": launch, "confidence": 0.9 }],
        }],
        "blackOuts": [{ "firstLine": 1, "lastLine": 1 }],
        "tasks": [
            task(2, 2, "Fix the login bug on the launch page", &grace, Some(launch)),
            task(3, 3, "Send Ada the deck", &tester, None),
            task(4, 4, "Book the room", "Sharath", None),
            task(5, 5, "Ask legal", "", None),
            task(release_notes.0, release_notes.1, "Write the release notes", &grace, None),
            task(7, 7, "Pick the launch date", launch_date_owed_by, Some(launch)),
        ],
    })
    .to_string()
}

async fn add_grace(connection: &mut AsyncPgConnection) {
    diesel::sql_query(
        "INSERT INTO users (id, kratos_identity_id, email, name, role, status, approved_at) \
         VALUES ($1, gen_random_uuid(), 'grace@example.com', 'Grace Hopper', 'member', 'active', \
         now())",
    )
    .bind::<diesel::sql_types::Uuid, _>(GRACE_ID)
    .execute(connection)
    .await
    .expect("a coworker");
}

/// Segments `heard` with `answer`, as its run's last step does.
async fn segment(state: &AppState, heard: &(Meeting, MeetingCapture), answer: &str) {
    let input = TranscriptToSegment::Meeting(CorrectedMeeting {
        meeting_id: heard.0.id,
        capture_id: Some(heard.1.id),
        transcript_hash: heard.1.transcript_hash.clone(),
        recording: None,
        whisper_transcript_hash: None,
        corrected_transcript_hash: None,
        approved: false,
    });
    let done = SegmentMeeting::answering(state.database.clone(), &[answer])
        .run(
            input,
            StepContext::for_test::<SegmentMeeting>(state.database.clone()),
        )
        .await
        .expect("segments");
    assert!(matches!(done, Done::Succeeded { .. }), "{done:?}");
}

/// Sends `method` to `path` as the signed-in test person: its status and body.
async fn send(state: &AppState, method: Method, path: &str) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::COOKIE, signed_in_cookie(state).await)
        .header(header::ORIGIN, "http://localhost:4000")
        .body(Body::empty())
        .expect("a request builds");
    let response = routes::router(state)
        .with_state(state.clone())
        .oneshot(request)
        .await
        .expect("the router answers");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("the body reads");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// What the meeting's bar reads: its tasks, in the order they were said.
async fn bar(state: &AppState, meeting_id: Uuid) -> Vec<Value> {
    let (status, body) = send(
        state,
        Method::GET,
        &format!("/api/v1/meetings/{meeting_id}/tasks"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["tasks"].as_array().expect("tasks").clone()
}

fn titled<'tasks>(tasks: &'tasks [Value], title: &str) -> &'tasks Value {
    tasks
        .iter()
        .find(|task| task["title"] == title)
        .unwrap_or_else(|| panic!("a task titled {title}"))
}

/// Calls the turn's tool `tool` with `arguments` over the agent-tools endpoint, as the turn's
/// Project Manager does: whether it was refused, and what it answered.
async fn call(state: &AppState, bearer: &str, tool: &str, arguments: Value) -> (bool, Value) {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": tool, "arguments": arguments },
    });
    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/api/agent-tools/{}",
            crate::tools::project_manager::SERVER.name
        ))
        .header(header::HOST, "api:8080")
        .header(header::AUTHORIZATION, bearer)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .body(Body::from(body.to_string()))
        .expect("a request builds");
    let response = routes::router(state)
        .with_state(state.clone())
        .oneshot(request)
        .await
        .expect("the router answers");
    assert_eq!(response.status(), StatusCode::OK, "{tool}");
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("the body reads");
    let answer: Value = serde_json::from_slice(&bytes).expect("the server answers JSON");
    let result = &answer["result"];
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("{tool} answers text: {answer}"));
    let refused = result["isError"] == true;
    (
        refused,
        serde_json::from_str(text).unwrap_or_else(|_| json!(text)),
    )
}

/// The project's turns so far, run by a dispatcher whose runner answers each turn as done.
struct Turns {
    context: DispatchContext,
    script: Arc<Mutex<Script>>,
}

impl Turns {
    fn new(state: &AppState) -> Self {
        let done = ElysiaOutcome::Completed {
            result: Some("Answered the meeting's tasks.\nSummary: Answered its tasks.".to_owned()),
            session_id: Some("session-1".to_owned()),
        };
        let script = Arc::new(Mutex::new(Script {
            outcomes: VecDeque::from(vec![done.clone(), done]),
            asked: Vec::new(),
        }));
        let context = DispatchContext {
            database: state.database.clone(),
            events: EventBus::in_process(),
            runner: ManagerRunner::Scripted(Arc::clone(&script)),
            grants: state.grants.clone(),
        };
        Self { context, script }
    }

    /// Lets every waiting wake go quiet and runs the turn that is then due: what it was asked.
    async fn next(&self, connection: &mut AsyncPgConnection) -> ManagerTurn {
        diesel::update(project_manager_wakes::table)
            .set(project_manager_wakes::created_at.eq(Utc::now() - TimeDelta::minutes(5)))
            .execute(connection)
            .await
            .expect("backdated");
        let before = self.script.lock().expect("script").asked.len();
        let mut running = tokio::task::JoinSet::new();
        dispatcher::dispatch(&self.context, &mut running, &CancellationToken::new())
            .await
            .expect("the dispatcher looks");
        while running.join_next().await.is_some() {}
        let asked = self.script.lock().expect("script").asked.clone();
        assert_eq!(asked.len(), before + 1, "one turn ran");
        asked.last().expect("a turn").clone()
    }
}

/// A grant to the tools of the Project Manager's turn `turn_id` of `project_id`, and its bearer.
/// The grant is held for as long as the caller keeps it, as a running turn holds its own:
/// dropping it revokes it.
async fn turn_bearer(state: &AppState, project_id: Uuid, turn_id: Uuid) -> (Grant, String) {
    let scope = CallScope::ProjectManager(ProjectManagerScope {
        project_id,
        turn_id,
    });
    let grant = state
        .grants
        .grant(&scope, &[&crate::tools::project_manager::SERVER])
        .await
        .expect("a grant is stored");
    let McpServer::Http { headers, .. } = &grant.mcp_servers()[0] else {
        panic!("tools are served over HTTP");
    };
    let bearer = headers[0].1.expose_secret().to_owned();
    (grant, bearer)
}

async fn waiting_kinds(connection: &mut AsyncPgConnection, project_id: Uuid) -> Vec<WakeKind> {
    project_manager_wakes::table
        .filter(project_manager_wakes::project_id.eq(project_id))
        .filter(project_manager_wakes::turn_id.is_null())
        .select(project_manager_wakes::kind)
        .load(connection)
        .await
        .expect("wakes")
}

async fn task_items(connection: &mut AsyncPgConnection) -> Vec<(Uuid, Uuid, Option<Uuid>)> {
    meeting_task_items::table
        .select((
            meeting_task_items::action_item_id,
            meeting_task_items::task_id,
            meeting_task_items::person_id,
        ))
        .order(meeting_task_items::action_item_id.asc())
        .load(connection)
        .await
        .expect("links")
}

/// The meeting's tasks from segmenting to action items, with its launch project at `autonomy`.
#[expect(
    clippy::too_many_lines,
    reason = "one meeting from its first reading through a turn, Done reviewing, and the next turn"
)]
async fn drive(autonomy: Autonomy) {
    let (url, mut connection) = migrated_database().await;
    fetched_agent_configuration(&mut connection).await;
    let state = app_state(&url).await;
    add_grace(&mut connection).await;
    let launch = project::create(
        &mut connection,
        &NewProject {
            name: "Launch".to_owned(),
            description: String::new(),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("a project")
    .id;
    let person = test_member(&mut connection).await;
    project_autonomy::set(&mut connection, launch, autonomy, &person)
        .await
        .expect("the dial is set");
    let heard = fetched_with(
        &mut connection,
        serde_json::to_value(transcript()).expect("serializes"),
    )
    .await;
    let meeting_id = heard.0.id;

    // The first reading: two tasks for the launch, four on no project.
    segment(&state, &heard, &answer(launch, (6, 6), "")).await;
    let tasks = bar(&state, meeting_id).await;
    let shares: Vec<&Value> = tasks.iter().map(|task| &task["share"]).collect();
    assert_eq!(
        shares,
        [
            &json!({ "kind": "project", "projectId": launch }),
            &json!({ "kind": "own" }),
            &json!({ "kind": "waiting-on", "name": "Sharath" }),
            &json!({ "kind": "other" }),
            &json!({ "kind": "coworker", "userId": GRACE_ID, "name": "Grace Hopper" }),
            &json!({ "kind": "project", "projectId": launch }),
        ],
        "in the order they were said, a project's first, then whoever owes it"
    );
    // Everyone else's became an inbox item at once, each for whom it is, linked to its task.
    let linked = task_items(&mut connection).await;
    assert_eq!(linked.len(), 4, "{linked:?}");
    let mut notes = Vec::new();
    for (item_id, task_id, person_id) in &linked {
        let item = action_item::find(&mut connection, *item_id)
            .await
            .expect("an item");
        assert_eq!(item.state, ActionItemState::Inbox);
        assert_eq!(item.created_by, crate::models::user::ELYSIA_USER_ID);
        let task = tasks
            .iter()
            .find(|task| task["id"] == json!(task_id))
            .expect("its task is on the bar");
        assert_eq!(task["items"][0]["id"], json!(item_id));
        let waiting = (item.title == "Book the room").then_some("Sharath");
        assert_eq!(item.waiting_on.as_deref(), waiting, "{}", item.title);
        let for_grace = item.title == "Write the release notes";
        assert_eq!(
            *person_id,
            Some(if for_grace { GRACE_ID } else { TEST_PERSON_ID })
        );
        notes.push(item.notes);
        let (status, opened) = send(
            &state,
            Method::GET,
            &format!("/api/v1/meetings/task-items/{item_id}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            opened["task"],
            json!({ "meetingId": meeting_id, "meetingTitle": heard.0.title, "taskId": task_id }),
            "each item links back to its range"
        );
    }
    assert!(
        notes
            .iter()
            .any(|note| note.starts_with("For Grace Hopper.")),
        "{notes:?}"
    );
    assert!(
        notes
            .iter()
            .any(|note| note.starts_with("For Tester, waiting on Sharath.")),
        "{notes:?}"
    );
    let mut waiting = waiting_kinds(&mut connection, launch).await;
    waiting.sort_by_key(|kind| format!("{kind:?}"));
    assert_eq!(
        waiting,
        [WakeKind::Record, WakeKind::Task, WakeKind::Task],
        "the excerpt and its two tasks wake the launch's Project Manager"
    );

    // Its turn reads each task with its words and the excerpt around it, never the meeting.
    let turns = Turns::new(&state);
    let first = turns.next(&mut connection).await;
    let login = titled(&tasks, "Fix the login bug on the launch page");
    let date = titled(&tasks, "Pick the launch date");
    let excerpt = timeline_record::of_meeting(&mut connection, meeting_id)
        .await
        .expect("excerpts")
        .remove(0)
        .0;
    assert!(
        first.prompt.contains("## Tasks heard in meetings (2)"),
        "{}",
        first.prompt
    );
    assert!(
        first.prompt.contains(&format!(
            "\"Fix the login bug on the launch page\" (task {}), owed by Grace Hopper, a person on \
             Elysium; heard in",
            login["id"].as_str().expect("an id")
        )),
        "{}",
        first.prompt
    );
    assert!(
        first
            .prompt
            .contains("[1:00] Grace: I'll fix the login bug on the launch page."),
        "{}",
        first.prompt
    );
    assert!(
        first.prompt.contains(&format!(
            "In the excerpt \"The launch\" (record {})",
            excerpt.id
        )),
        "{}",
        first.prompt
    );
    assert!(
        !first.prompt.contains("Thanks everyone"),
        "the meeting is never read whole"
    );

    // As the turn, it makes the login bug an item and declines the launch date.
    let (grant, bearer) = turn_bearer(&state, launch, first.turn_id).await;
    let (refused, made) = call(
        &state,
        &bearer,
        "manager_make_changes",
        json!({
            "summary": "The login bug from the launch sync",
            "operations": [{
                "change": { "kind": "create-item", "title": "Fix the login bug on the launch page" },
                "reason": "Grace took it on in the launch sync.",
                "taskId": login["id"],
                "causeRecordId": excerpt.id,
            }],
        }),
    )
    .await;
    assert!(!refused, "{made}");
    let (refused, declined) = call(
        &state,
        &bearer,
        "manager_decline_task",
        json!({ "taskId": date["id"], "reason": "Nobody took it on yet; I asked in my reply." }),
    )
    .await;
    assert!(!refused, "{declined}");
    // A change of another kind may name the task it came from, and makes no item of it.
    let (refused, grouped) = call(
        &state,
        &bearer,
        "manager_make_changes",
        json!({
            "summary": "An initiative for the launch date",
            "operations": [{
                "change": { "kind": "create-initiative", "name": "Launch date" },
                "reason": "The date is still open.",
                "taskId": date["id"],
            }],
        }),
    )
    .await;
    assert!(!refused, "{grouped}");
    let not_its_own = titled(&tasks, "Send Ada the deck");
    let (refused, answer_text) = call(
        &state,
        &bearer,
        "manager_decline_task",
        json!({ "taskId": not_its_own["id"], "reason": "Not ours." }),
    )
    .await;
    assert!(
        refused,
        "a task on no project is not its to answer: {answer_text}"
    );
    grant.revoke().await;

    let changeset_id: Uuid =
        serde_json::from_value(made["changeset"]["id"].clone()).expect("a changeset");
    let staged = changeset::find(&mut connection, changeset_id)
        .await
        .expect("the changeset");
    if autonomy.writes_directly() {
        assert_eq!(
            staged.changeset.state,
            ChangesetState::Applied,
            "{autonomy:?}"
        );
    } else {
        // At every other level it waits for a person, and nothing is made until they approve.
        assert_eq!(
            staged.changeset.state,
            ChangesetState::Pending,
            "{autonomy:?}"
        );
        let tasks = bar(&state, meeting_id).await;
        let login = titled(&tasks, "Fix the login bug on the launch page");
        assert_eq!(login["proposals"], json!([changeset_id]));
        assert_eq!(login["items"], json!([]));
        changeset::decide(
            &mut connection,
            changeset_id,
            ChangesetDecision::Approved,
            None,
        )
        .await
        .expect("approved");
        changeset::apply(
            &mut connection,
            changeset_id,
            LinkReads::new(),
            TEST_PERSON_ID,
            Utc::now(),
        )
        .await
        .expect("applied");
    }
    let tasks = bar(&state, meeting_id).await;
    let login = titled(&tasks, "Fix the login bug on the launch page");
    assert_eq!(login["proposals"], json!([]));
    let item_id: Uuid = serde_json::from_value(login["items"][0]["id"].clone()).expect("an item");
    let item = action_item::find(&mut connection, item_id)
        .await
        .expect("the item");
    // A person's approval accepts it; applied unreviewed, it waits in the inbox.
    let expected = if autonomy.writes_directly() {
        ActionItemState::Inbox
    } else {
        ActionItemState::Open
    };
    assert_eq!(item.state, expected, "{autonomy:?}");
    let date_now = titled(&tasks, "Pick the launch date");
    assert_eq!(
        date_now["declined"]["reason"],
        "Nobody took it on yet; I asked in my reply."
    );
    assert_eq!(
        date_now["proposals"],
        json!([]),
        "an initiative naming the task proposes no item of it"
    );
    let items_before = action_items::table
        .count()
        .get_result::<i64>(&mut connection)
        .await
        .expect("counts");

    // Done reviewing reads the meeting again: the release notes are heard over a line more, and
    // the launch date now has Grace on it. Only that one changed.
    meeting::reopen_for_segmenting(&mut connection, meeting_id)
        .await
        .expect("reopens");
    let release_notes_before = titled(&tasks, "Write the release notes")["id"].clone();
    segment(
        &state,
        &heard,
        &answer(launch, (6, 7), &GRACE_ID.to_string()),
    )
    .await;
    let tasks = bar(&state, meeting_id).await;
    assert_eq!(tasks.len(), 6);
    let release_notes = titled(&tasks, "Write the release notes");
    assert_eq!(
        release_notes["id"], release_notes_before,
        "the same task, moved onto its words"
    );
    assert_eq!(
        release_notes["endWordId"],
        tasks
            .iter()
            .find(|task| task["title"] == "Pick the launch date")
            .expect("the launch date")["endWordId"],
        "it now ends where the launch date does"
    );
    assert_eq!(
        action_items::table
            .count()
            .get_result::<i64>(&mut connection)
            .await
            .expect("counts"),
        items_before,
        "no item is raised twice"
    );
    assert_eq!(
        waiting_kinds(&mut connection, launch).await,
        [WakeKind::Task],
        "only the task that changed reaches the Project Manager"
    );
    assert_eq!(
        meeting_mark::standing(&mut connection, meeting_id)
            .await
            .expect("marks")
            .iter()
            .filter(|mark| mark.kind() == Some(MarkKind::Task))
            .count(),
        6
    );

    // Its next turn reads that task, with what came of it before, and nothing else.
    let second = turns.next(&mut connection).await;
    assert!(
        second.prompt.contains("## Tasks heard in meetings (1)"),
        "{}",
        second.prompt
    );
    assert!(
        second.prompt.contains(&format!(
            "\"Pick the launch date\" (task {}), owed by Grace Hopper, a person on Elysium; heard \
             in \"Weekly sync\"",
            date["id"].as_str().expect("an id")
        )),
        "{}",
        second.prompt
    );
    assert!(
        second
            .prompt
            .contains("You made no item of it before, saying: Nobody took it on yet"),
        "{}",
        second.prompt
    );
    assert!(
        !second.prompt.contains("Fix the login bug"),
        "{}",
        second.prompt
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn at_approve_every_batch_a_meetings_tasks_wait_for_a_person() {
    drive(Autonomy::ApproveEveryBatch).await;
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn at_approve_found_work_a_meetings_tasks_still_wait_for_a_person() {
    drive(Autonomy::ApproveFoundWork).await;
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn at_full_autonomy_a_meetings_tasks_become_items_at_once() {
    drive(Autonomy::FullAutonomy).await;
}
