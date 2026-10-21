// Copyright © 2026 Jalapeno Labs

//! `elysia`: Elysia's own tools, for her turns alone.
//!
//! She acts for the whole workspace, so nothing narrows what she reads: every live action
//! item, initiative, and project. What she writes is her own record: the questions she asks
//! a person and the work she has taken on, which her panel shows the moment she writes it.
//! Changes to items and initiatives are proposed as changesets, which arrive with the next
//! set of tools; she cannot change them directly.
//!
//! Each tool's database work is a function of a connection, so it is tested against Postgres
//! without a turn; the `run_*` wrappers only parse, connect, and announce.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use futures_util::FutureExt as _;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{CallScope, Tool, ToolContext, ToolError, ToolServer, internal, parse_arguments};
use crate::models::action_item::{self, ActionItemFilter, ActionItemState};
use crate::models::elysia::{self, NewQuestion, NewWork, WorkChanges, WorkStatus};
use crate::models::initiative::{self, InitiativeFilter, InitiativeState};
use crate::models::user::{self, ELYSIA_USER_ID};
use crate::models::{action_item_comment, project};
use crate::routes::v1::elysia::publish_snapshot;

pub const SERVER: ToolServer = ToolServer {
    name: "elysia",
    instructions: "Elysia's tools: the workspace's action items, initiatives, and projects to \
        read, and her own record to write. Start with elysia_overview. When you take something \
        on, record it with elysia_work_start, including when you expect it ready for review, \
        and keep it current with elysia_work_update: ready when a person should review it, \
        blocked when you cannot go on, gave-up when you stop trying, done when nothing is left. \
        When only a person can unblock you, ask with elysia_ask; their answer starts your next \
        turn. People see all of this in your panel as soon as you write it.",
    tools: &TOOLS,
};

const TOOLS: [Tool; 5] = [
    Tool {
        name: "elysia_overview",
        description: "Shows the workspace: every action item in the inbox or open, newest \
            first, without notes; the active initiatives; the projects; and your own open \
            questions and unfinished work.",
        input_schema: no_arguments_schema,
        run: |context, scope, arguments| run_overview(context, scope, arguments).boxed(),
    },
    Tool {
        name: "elysia_item",
        description: "Shows one action item in full: its notes and every comment.",
        input_schema: item_schema,
        run: |context, scope, arguments| run_item(context, scope, arguments).boxed(),
    },
    Tool {
        name: "elysia_ask",
        description: "Asks a person one clear question, with up to four suggested answers, \
            when only they can unblock you. It shows in your panel at once; their answer starts \
            your next turn. Never ask what you can find out yourself.",
        input_schema: ask_schema,
        run: |context, scope, arguments| run_ask(context, scope, arguments).boxed(),
    },
    Tool {
        name: "elysia_work_start",
        description: "Records something you are taking on: a short title, what you will do, \
            and when you expect it ready for review. Answers its workId.",
        input_schema: work_start_schema,
        run: |context, scope, arguments| run_work_start(context, scope, arguments).boxed(),
    },
    Tool {
        name: "elysia_work_update",
        description: "Updates your record of a piece of work: its title, where it stands, its \
            status, or when you expect it ready (null clears the estimate).",
        input_schema: work_update_schema,
        run: |context, scope, arguments| run_work_update(context, scope, arguments).boxed(),
    },
];

/// The most items the overview lists. A workspace's open work fits; she reads one with
/// `elysia_item` for its full record.
const OVERVIEW_ITEMS_MAX: usize = 200;

/// Matching `elysia_questions`' limits.
const PROMPT_MAX_CHARACTERS: usize = 2_000;
const OPTIONS_MAX: usize = 4;

/// Matching `elysia_work`'s limits.
const TITLE_MAX_CHARACTERS: usize = 200;
const DETAIL_MAX_CHARACTERS: usize = 2_000;

fn no_arguments_schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn item_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "itemId": { "type": "string", "format": "uuid" } },
        "required": ["itemId"],
        "additionalProperties": false,
    })
}

fn ask_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "prompt": { "type": "string", "minLength": 1, "maxLength": PROMPT_MAX_CHARACTERS },
            "options": {
                "type": "array",
                "items": { "type": "string", "minLength": 1 },
                "maxItems": OPTIONS_MAX,
                "description": "Suggested answers; a person may also answer in their own words.",
            },
            "actionItemId": { "type": "string", "format": "uuid", "description": "The item it is about." },
        },
        "required": ["prompt"],
        "additionalProperties": false,
    })
}

fn work_start_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "minLength": 1, "maxLength": TITLE_MAX_CHARACTERS },
            "detail": { "type": "string", "maxLength": DETAIL_MAX_CHARACTERS },
            "readyAt": {
                "type": "string",
                "format": "date-time",
                "description": "When you expect it ready for review, with a UTC offset.",
            },
            "actionItemId": { "type": "string", "format": "uuid", "description": "The item it serves." },
        },
        "required": ["title"],
        "additionalProperties": false,
    })
}

fn work_update_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workId": { "type": "string", "format": "uuid" },
            "title": { "type": "string", "minLength": 1, "maxLength": TITLE_MAX_CHARACTERS },
            "detail": { "type": "string", "maxLength": DETAIL_MAX_CHARACTERS },
            "status": { "enum": ["working", "ready", "blocked", "gave-up", "done"] },
            "readyAt": { "type": ["string", "null"], "format": "date-time" },
        },
        "required": ["workId"],
        "additionalProperties": false,
    })
}

#[expect(
    clippy::empty_structs_with_brackets,
    reason = "serde reads {} only into a braced struct; a unit struct takes null"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArguments {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ItemArguments {
    item_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AskArguments {
    prompt: String,
    #[serde(default)]
    options: Vec<String>,
    action_item_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkStartArguments {
    title: String,
    #[serde(default)]
    detail: String,
    ready_at: Option<DateTime<Utc>>,
    action_item_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkUpdateArguments {
    work_id: Uuid,
    title: Option<String>,
    detail: Option<String>,
    status: Option<WorkStatus>,
    /// Absent leaves the estimate; `null` clears it.
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[expect(
        clippy::option_option,
        reason = "absent leaves the estimate untouched and null clears it"
    )]
    ready_at: Option<Option<DateTime<Utc>>>,
}

/// Refuses text that is blank or longer than `max` characters, naming `field`.
fn check_text(field: &str, text: &str, max: usize) -> Result<(), ToolError> {
    if text.trim().is_empty() {
        return Err(ToolError::Invalid(format!("{field} must not be blank")));
    }
    if text.chars().count() > max {
        return Err(ToolError::Invalid(format!(
            "{field} is longer than {max} characters"
        )));
    }
    Ok(())
}

async fn run_overview(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    scope.elysia()?;
    let NoArguments {} = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    overview(&mut connection, Utc::now())
        .await
        .map_err(|error| internal(scope, "elysia.overview", &error))
}

/// The workspace at a glance, for [`run_overview`].
async fn overview(
    connection: &mut AsyncPgConnection,
    now: DateTime<Utc>,
) -> diesel::QueryResult<Value> {
    let items = action_item::list(
        connection,
        &ActionItemFilter {
            states: vec![ActionItemState::Inbox, ActionItemState::Open],
            project: None,
            initiative: None,
            waiting: None,
            snoozed: None,
            deleted: false,
        },
        now,
    )
    .await?;
    let item_ids: Vec<Uuid> = items.iter().map(|item| item.id).collect();
    let memberships = action_item::memberships(connection, &item_ids).await?;
    let initiatives = initiative::list(
        connection,
        &InitiativeFilter {
            states: vec![InitiativeState::Active],
            project: None,
            deleted: false,
        },
    )
    .await?;
    let projects = project::list(connection).await?;
    let questions = elysia::open_questions(connection).await?;
    let work = elysia::unfinished_work(connection).await?;

    let more_items = items.len() > OVERVIEW_ITEMS_MAX;
    let items: Vec<Value> = items
        .iter()
        .take(OVERVIEW_ITEMS_MAX)
        .map(|item| {
            json!({
                "id": item.id,
                "title": item.title,
                "state": item.state,
                "priority": item.priority,
                "dueAt": item.due_at,
                "waitingOn": item.waiting_on,
                "projectIds": memberships.project_ids(item.id),
                "initiativeIds": memberships.initiative_ids(item.id),
                "createdAt": item.created_at,
            })
        })
        .collect();

    Ok(json!({
        "items": items,
        "moreItems": more_items,
        "initiatives": initiatives.iter().map(|initiative| json!({
            "id": initiative.id,
            "name": initiative.name,
            "description": initiative.description,
            "targetAt": initiative.target_at,
        })).collect::<Vec<Value>>(),
        "projects": projects.iter().map(|project| json!({
            "id": project.id,
            "name": project.name,
            "description": project.description,
        })).collect::<Vec<Value>>(),
        "openQuestions": questions.iter().map(|question| json!({
            "id": question.id,
            "prompt": question.prompt,
            "actionItemId": question.action_item_id,
        })).collect::<Vec<Value>>(),
        "work": work.iter().map(|item| json!({
            "id": item.id,
            "title": item.title,
            "status": item.status,
            "detail": item.detail,
            "readyAt": item.ready_at,
            "actionItemId": item.action_item_id,
        })).collect::<Vec<Value>>(),
    }))
}

async fn run_item(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    scope.elysia()?;
    let ItemArguments { item_id } = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    item(&mut connection, item_id)
        .await
        .map_err(|failure| match failure {
            ItemFailure::Missing => ToolError::NotFound(format!(
                "action item {item_id} does not exist or was deleted; call elysia_overview for the \
             live ones"
            )),
            ItemFailure::Database(error) => internal(scope, "elysia.item", &error),
        })
}

/// Why an item could not be shown.
#[derive(Debug)]
enum ItemFailure {
    Missing,
    Database(diesel::result::Error),
}

/// One live item with its notes and comments, for [`run_item`].
async fn item(connection: &mut AsyncPgConnection, item_id: Uuid) -> Result<Value, ItemFailure> {
    let found = action_item::find(connection, item_id)
        .await
        .map_err(|error| match error {
            diesel::result::Error::NotFound => ItemFailure::Missing,
            error => ItemFailure::Database(error),
        })?;
    if found.deleted_at.is_some() {
        return Err(ItemFailure::Missing);
    }
    let comments = action_item_comment::list(connection, item_id)
        .await
        .map_err(ItemFailure::Database)?;
    let names: HashMap<Uuid, String> = user::names(connection)
        .await
        .map_err(ItemFailure::Database)?;
    let memberships = action_item::memberships(connection, &[item_id])
        .await
        .map_err(ItemFailure::Database)?;

    Ok(json!({
        "id": found.id,
        "title": found.title,
        "notes": found.notes,
        "state": found.state,
        "priority": found.priority,
        "dueAt": found.due_at,
        "waitingOn": found.waiting_on,
        "projectIds": memberships.project_ids(item_id),
        "initiativeIds": memberships.initiative_ids(item_id),
        "createdAt": found.created_at,
        "comments": comments.iter().map(|comment| json!({
            "author": comment.author,
            "authorName": comment
                .author
                .strip_prefix("user:")
                .and_then(|id| id.parse::<Uuid>().ok())
                .and_then(|id| names.get(&id).cloned()),
            "body": comment.body,
            "createdAt": comment.created_at,
        })).collect::<Vec<Value>>(),
    }))
}

async fn run_ask(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    scope.elysia()?;
    let arguments: AskArguments = parse_arguments(arguments)?;
    check_text("prompt", &arguments.prompt, PROMPT_MAX_CHARACTERS)?;
    if arguments.options.len() > OPTIONS_MAX {
        return Err(ToolError::Invalid(format!(
            "offer at most {OPTIONS_MAX} suggested answers"
        )));
    }
    for option in &arguments.options {
        check_text("each option", option, PROMPT_MAX_CHARACTERS)?;
    }

    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let question = elysia::insert_question(
        &mut connection,
        NewQuestion {
            id: Uuid::now_v7(),
            prompt: arguments.prompt,
            options: json!(arguments.options),
            action_item_id: arguments.action_item_id,
            created_by: ELYSIA_USER_ID,
        },
    )
    .await
    .map_err(|error| refused_or_internal(scope, "elysia.ask", &error))?;
    publish_snapshot(&context.events, &mut connection)
        .await
        .map_err(|error| internal(scope, "elysia.snapshot", &error))?;

    Ok(json!({
        "questionId": question.id,
        "note": "Asked. A person sees it in your panel now, and their answer starts your next turn.",
    }))
}

async fn run_work_start(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    scope.elysia()?;
    let arguments: WorkStartArguments = parse_arguments(arguments)?;
    check_text("title", &arguments.title, TITLE_MAX_CHARACTERS)?;
    if arguments.detail.chars().count() > DETAIL_MAX_CHARACTERS {
        return Err(ToolError::Invalid(format!(
            "detail is longer than {DETAIL_MAX_CHARACTERS} characters"
        )));
    }

    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let work = elysia::insert_work(
        &mut connection,
        NewWork {
            id: Uuid::now_v7(),
            title: arguments.title,
            detail: arguments.detail,
            ready_at: arguments.ready_at,
            action_item_id: arguments.action_item_id,
            created_by: ELYSIA_USER_ID,
        },
    )
    .await
    .map_err(|error| refused_or_internal(scope, "elysia.work_start", &error))?;
    publish_snapshot(&context.events, &mut connection)
        .await
        .map_err(|error| internal(scope, "elysia.snapshot", &error))?;

    Ok(json!({ "workId": work.id, "status": work.status }))
}

async fn run_work_update(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    scope.elysia()?;
    let arguments: WorkUpdateArguments = parse_arguments(arguments)?;
    if let Some(title) = &arguments.title {
        check_text("title", title, TITLE_MAX_CHARACTERS)?;
    }
    if arguments
        .detail
        .as_ref()
        .is_some_and(|detail| detail.chars().count() > DETAIL_MAX_CHARACTERS)
    {
        return Err(ToolError::Invalid(format!(
            "detail is longer than {DETAIL_MAX_CHARACTERS} characters"
        )));
    }

    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let Some(work) = elysia::update_work(
        &mut connection,
        arguments.work_id,
        WorkChanges {
            title: arguments.title,
            detail: arguments.detail,
            status: arguments.status,
            ready_at: arguments.ready_at,
        },
    )
    .await
    .map_err(|error| refused_or_internal(scope, "elysia.work_update", &error))?
    else {
        return Err(ToolError::NotFound(format!(
            "no work {} is recorded; call elysia_overview for yours",
            arguments.work_id
        )));
    };
    publish_snapshot(&context.events, &mut connection)
        .await
        .map_err(|error| internal(scope, "elysia.snapshot", &error))?;

    Ok(json!({ "workId": work.id, "status": work.status }))
}

/// A refused foreign key (an item that does not exist) is hers to fix; anything else is
/// Elysium's and is logged.
fn refused_or_internal(
    scope: &CallScope,
    operation: &'static str,
    error: &diesel::result::Error,
) -> ToolError {
    use diesel::result::{DatabaseErrorKind, Error};

    if let Error::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, _) = error {
        return ToolError::Invalid(
            "the action item named does not exist; call elysia_overview for the live ones"
                .to_owned(),
        );
    }
    internal(scope, operation, error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::migrated_database;

    #[test]
    fn a_null_estimate_clears_and_an_absent_one_stays() {
        let id = Uuid::nil();
        let cleared: WorkUpdateArguments =
            serde_json::from_value(json!({ "workId": id, "readyAt": null })).expect("parses");
        assert_eq!(cleared.ready_at, Some(None));

        let untouched: WorkUpdateArguments =
            serde_json::from_value(json!({ "workId": id })).expect("parses");
        assert_eq!(untouched.ready_at, None);
    }

    #[test]
    fn blank_and_overlong_text_is_refused() {
        assert!(check_text("title", "  ", TITLE_MAX_CHARACTERS).is_err());
        assert!(
            check_text(
                "title",
                &"x".repeat(TITLE_MAX_CHARACTERS + 1),
                TITLE_MAX_CHARACTERS
            )
            .is_err()
        );
        check_text("title", "Quarterly deck", TITLE_MAX_CHARACTERS).expect("valid text");
    }

    #[test]
    fn only_elysia_may_call_these_tools() {
        use crate::runner::WorkspacePath;
        use crate::tools::SessionScope;

        let session = CallScope::Session(SessionScope {
            session_id: 3,
            project_id: None,
            action_item_id: None,
            directory: WorkspacePath::new("sessions/3").expect("a path"),
        });
        assert_eq!(
            session.elysia().err(),
            Some(ToolError::WrongAgent("Elysia"))
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run scripts/verify-migrations.sh"]
    async fn the_overview_lists_open_work_and_her_own_record() {
        let (_url, mut connection) = migrated_database().await;
        elysia::insert_work(
            &mut connection,
            NewWork {
                id: Uuid::now_v7(),
                title: "Quarterly deck".to_owned(),
                detail: String::new(),
                ready_at: None,
                action_item_id: None,
                created_by: ELYSIA_USER_ID,
            },
        )
        .await
        .expect("work is recorded");

        let answer = overview(&mut connection, Utc::now())
            .await
            .expect("the overview loads");
        assert_eq!(answer["items"], json!([]));
        assert_eq!(answer["work"][0]["title"], json!("Quarterly deck"));
        assert_eq!(answer["moreItems"], json!(false));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run scripts/verify-migrations.sh"]
    async fn a_missing_item_is_hers_to_fix() {
        let (_url, mut connection) = migrated_database().await;
        assert!(matches!(
            item(&mut connection, Uuid::now_v7()).await,
            Err(ItemFailure::Missing)
        ));
    }
}
