// Copyright © 2026 Jalapeno Labs

//! `elysium_work`: the agent's tools for its project's action items and initiatives.
//!
//! The tools speak Elysium's terms (items, initiatives, projects, comments) and never a
//! provider's. Every call is scoped to the session's project and checked against the
//! database as it is now: an item or initiative taken out of the project or deleted after
//! the session started is refused, and one added later is reachable.
//!
//! The agent reads freely and writes only comments, recorded with the actor
//! `session:<number>`. Anything else it wants changed (creating, resolving, dismissing,
//! deleting, linking) is proposed to the user as a changeset instead, which is a later
//! stage: a `work_propose_changes` tool joins [`TOOLS`] then, and the instructions below
//! already tell the agent to ask the user in the meantime. See `docs/action-items.md`.
//!
//! Each tool's database work is a function of a connection and a [`WorkScope`], so it is
//! tested against Postgres without a satellite; the `run_*` wrappers only parse, connect,
//! and report.

use chrono::{DateTime, Utc};
use diesel::result::Error as DieselError;
use diesel_async::AsyncPgConnection;
use futures_util::FutureExt as _;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{CallScope, Tool, ToolContext, ToolError, ToolServer, internal, parse_arguments};
use crate::action_items::progress::{self, Progress};
use crate::action_items::{Actor, WorkError};
use crate::models::action_item::{
    self, ActionItem, ActionItemFilter, ActionItemState, Memberships, ProjectFilter,
};
use crate::models::action_item_comment::{self, Comment};
use crate::models::action_item_event::{self, HistoryEntry, Recorded};
use crate::models::initiative::{self, Initiative, InitiativeFilter, InitiativeState};
use crate::models::project;
use crate::realtime::ServerEvent;
use crate::routes::v1::action_items::{CommentResponse, HistoryEntryResponse};

pub const SERVER: ToolServer = ToolServer {
    name: "elysium_work",
    instructions: "The user's action items and initiatives for this session's project, as \
        Elysium tracks them. An action item is one commitment of the user's attention (fix, \
        review, reply, decide); an initiative groups items toward a goal that ends, and its \
        progress is resolved items out of the items that count. Start with work_project: it \
        shows the project, its open initiatives and items, and the item this session was \
        started from, if any. Use work_item and work_initiative for the full record, and \
        work_items to search. Every id you pass must belong to this project. \
        work_comment is the only write: use it to leave the user a short, useful note on an \
        item, such as what you found, what you changed, or the pull request you opened. \
        You cannot create, resolve, dismiss, or delete items or initiatives; when one \
        should change, say so to the user in your reply, and they will make the change.",
    tools: &TOOLS,
};

const TOOLS: [Tool; 6] = [
    Tool {
        name: "work_project",
        description: "Shows this session's project: its name and description, its active \
            initiatives with progress, its items waiting in the inbox or open, and \
            sessionItemId, the item this session was started from (null when none).",
        input_schema: no_arguments_schema,
        run: |context, scope, arguments| run_project(context, scope, arguments).boxed(),
    },
    Tool {
        name: "work_items",
        description: "Lists the project's action items, newest first, without their notes. \
            By default only items in the inbox or open; pass states for others. Filter by \
            initiative, by waiting on someone, or by text in the title or notes. Answers at \
            most limit items and whether more matched.",
        input_schema: items_schema,
        run: |context, scope, arguments| run_items(context, scope, arguments).boxed(),
    },
    Tool {
        name: "work_item",
        description: "Shows one action item in full: its notes, every comment, its latest \
            history, its projects, and its initiatives with their progress.",
        input_schema: item_schema,
        run: |context, scope, arguments| run_item(context, scope, arguments).boxed(),
    },
    Tool {
        name: "work_initiatives",
        description: "Lists the project's initiatives with their progress, by name. By \
            default only active ones; pass states for achieved or abandoned ones.",
        input_schema: initiatives_schema,
        run: |context, scope, arguments| run_initiatives(context, scope, arguments).boxed(),
    },
    Tool {
        name: "work_initiative",
        description: "Shows one initiative: its description, target date, progress, and the \
            items of this project that belong to it, in every state, newest first, with \
            moreItems when there are more than one answer holds.",
        input_schema: initiative_schema,
        run: |context, scope, arguments| run_initiative(context, scope, arguments).boxed(),
    },
    Tool {
        name: "work_comment",
        description: "Writes a comment on an action item, shown to the user in the item's \
            comments and history as written by this session. Comments cannot be edited or \
            deleted afterwards, so write each one complete.",
        input_schema: comment_schema,
        run: |context, scope, arguments| run_comment(context, scope, arguments).boxed(),
    },
];

/// The most items one answer lists. Enough for a project's open work; a longer list is
/// narrowed with filters rather than paged.
const ITEMS_MAX: usize = 200;

/// How many items `work_items` lists when the call does not say.
const ITEMS_DEFAULT: usize = 50;

/// The most history entries `work_item` shows, newest kept. An item's history grows with
/// every edit; the latest entries are what explain its state.
const HISTORY_MAX: usize = 50;

/// The longest comment an agent may write, matching the database's limit.
const COMMENT_MAX_CHARACTERS: usize = 20_000;

/// What a call reaches: the session and its project, and the item it was started from.
/// The parts of [`CallScope`] that need no satellite, so the queries are testable alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_field_names,
    reason = "each field is an id, named as on CallScope and the session"
)]
struct WorkScope {
    session_id: i64,
    project_id: Uuid,
    action_item_id: Option<Uuid>,
}

impl From<&CallScope> for WorkScope {
    fn from(scope: &CallScope) -> Self {
        Self {
            session_id: scope.session_id,
            project_id: scope.project_id,
            action_item_id: scope.action_item_id,
        }
    }
}

/// Why a query could not answer: something the agent can fix, or a fault inside Elysium,
/// which is logged and never shown to the agent.
#[derive(Debug)]
enum Failure {
    Refused(ToolError),
    Database(DieselError),
}

impl From<ToolError> for Failure {
    fn from(error: ToolError) -> Self {
        Self::Refused(error)
    }
}

impl From<DieselError> for Failure {
    fn from(error: DieselError) -> Self {
        Self::Database(error)
    }
}

impl From<WorkError> for Failure {
    fn from(error: WorkError) -> Self {
        match error {
            WorkError::Database(database) => Self::Database(database),
            WorkError::Conflict(message) | WorkError::Invalid(message) => {
                Self::Refused(ToolError::Invalid(message.to_owned()))
            }
        }
    }
}

impl Failure {
    fn into_tool_error(self, scope: &CallScope, operation: &'static str) -> ToolError {
        match self {
            Self::Refused(error) => error,
            Self::Database(error) => internal(scope, operation, &error),
        }
    }
}

fn no_arguments_schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn id_property(description: &str) -> Value {
    json!({ "type": "string", "format": "uuid", "description": description })
}

/// A list of states to filter on, as a schema property.
fn states_property(states: &[&str], default: &str) -> Value {
    json!({
        "type": "array",
        "items": { "type": "string", "enum": states },
        "minItems": 1,
        "uniqueItems": true,
        "description": format!("Only these states. Omit for {default}."),
    })
}

fn items_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "states": states_property(
                &["inbox", "open", "resolved", "dismissed"],
                "inbox and open",
            ),
            "initiativeId": id_property("Only items in this initiative, from work_initiatives."),
            "waiting": {
                "type": "boolean",
                "description": "true for only items waiting on someone else, false for none.",
            },
            "search": {
                "type": "string",
                "minLength": 1,
                "maxLength": 200,
                "description": "Only items whose title or notes contain this text, ignoring case.",
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": ITEMS_MAX,
                "description": format!("The most items to list. Omit for {ITEMS_DEFAULT}."),
            },
        },
        "additionalProperties": false,
    })
}

fn item_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "itemId": id_property("The item's id.") },
        "required": ["itemId"],
        "additionalProperties": false,
    })
}

fn initiatives_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "states": states_property(&["active", "achieved", "abandoned"], "active only"),
        },
        "additionalProperties": false,
    })
}

fn initiative_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "initiativeId": id_property("The initiative's id.") },
        "required": ["initiativeId"],
        "additionalProperties": false,
    })
}

fn comment_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "itemId": id_property("The item to comment on."),
            "body": {
                "type": "string",
                "minLength": 1,
                "maxLength": COMMENT_MAX_CHARACTERS,
                "description": "The comment, in plain text or Markdown.",
            },
        },
        "required": ["itemId", "body"],
        "additionalProperties": false,
    })
}

#[expect(
    clippy::empty_structs_with_brackets,
    reason = "serde reads {} only into a braced struct; a unit struct takes null"
)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NoArguments {}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ItemsArguments {
    states: Option<Vec<ActionItemState>>,
    initiative_id: Option<Uuid>,
    waiting: Option<bool>,
    search: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ItemArguments {
    item_id: Uuid,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InitiativesArguments {
    states: Option<Vec<InitiativeState>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InitiativeArguments {
    initiative_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CommentArguments {
    item_id: Uuid,
    body: String,
}

async fn run_project(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let NoArguments {} = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    project_overview(&mut connection, WorkScope::from(scope), Utc::now())
        .await
        .map_err(|failure| failure.into_tool_error(scope, "work.project"))
}

async fn run_items(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: ItemsArguments = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    list_items(
        &mut connection,
        WorkScope::from(scope),
        arguments,
        Utc::now(),
    )
    .await
    .map_err(|failure| failure.into_tool_error(scope, "work.items"))
}

async fn run_item(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: ItemArguments = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    item_detail(
        &mut connection,
        WorkScope::from(scope),
        arguments.item_id,
        Utc::now(),
    )
    .await
    .map_err(|failure| failure.into_tool_error(scope, "work.item"))
}

async fn run_initiatives(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: InitiativesArguments = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    list_initiatives(
        &mut connection,
        WorkScope::from(scope),
        arguments,
        Utc::now(),
    )
    .await
    .map_err(|failure| failure.into_tool_error(scope, "work.initiatives"))
}

async fn run_initiative(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: InitiativeArguments = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    initiative_detail(
        &mut connection,
        WorkScope::from(scope),
        arguments.initiative_id,
        Utc::now(),
    )
    .await
    .map_err(|failure| failure.into_tool_error(scope, "work.initiative"))
}

/// Writes the comment, then tells every client about it, as the comment route does.
async fn run_comment(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: CommentArguments = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let Recorded { record, history } = comment_on_item(
        &mut connection,
        WorkScope::from(scope),
        arguments,
        Utc::now(),
    )
    .await
    .map_err(|failure| failure.into_tool_error(scope, "work.comment"))?;
    drop(connection);

    for entry in history {
        context
            .events
            .publish(&ServerEvent::HistoryAppended(HistoryEntryResponse::from(
                entry,
            )));
    }
    let comment = CommentResponse::from(record);
    context
        .events
        .publish(&ServerEvent::ActionItemCommentUpserted(comment.clone()));
    Ok(json!({ "comment": comment }))
}

/// The project with its active initiatives and its items in the inbox or open.
async fn project_overview(
    connection: &mut AsyncPgConnection,
    scope: WorkScope,
    now: DateTime<Utc>,
) -> Result<Value, Failure> {
    let project = project::find(connection, scope.project_id).await?;
    let initiatives =
        initiatives_in_project(connection, scope, InitiativesArguments::default(), now).await?;
    let open_items = ItemsArguments {
        limit: Some(ITEMS_MAX),
        ..ItemsArguments::default()
    };
    let (items, more_items) = items_in_project(connection, scope, open_items, now).await?;

    Ok(json!({
        "project": {
            "id": project.id,
            "name": project.name,
            "description": project.description,
        },
        "sessionItemId": scope.action_item_id,
        "initiatives": initiatives,
        "items": items,
        "moreItems": more_items,
    }))
}

/// The project's items that `arguments` select, newest first.
async fn list_items(
    connection: &mut AsyncPgConnection,
    scope: WorkScope,
    arguments: ItemsArguments,
    now: DateTime<Utc>,
) -> Result<Value, Failure> {
    let (items, more_items) = items_in_project(connection, scope, arguments, now).await?;
    Ok(json!({ "items": items, "moreItems": more_items }))
}

/// The summaries of the project's items that `arguments` select, newest first, and whether
/// more matched than the limit lets through.
async fn items_in_project(
    connection: &mut AsyncPgConnection,
    scope: WorkScope,
    arguments: ItemsArguments,
    now: DateTime<Utc>,
) -> Result<(Vec<Value>, bool), Failure> {
    let limit = arguments.limit.unwrap_or(ITEMS_DEFAULT);
    if !(1..=ITEMS_MAX).contains(&limit) {
        return Err(ToolError::Invalid(format!("limit must be from 1 to {ITEMS_MAX}")).into());
    }
    if let Some(initiative_id) = arguments.initiative_id {
        find_initiative(connection, scope, initiative_id).await?;
    }
    let filter = ActionItemFilter {
        states: arguments
            .states
            .unwrap_or_else(|| vec![ActionItemState::Inbox, ActionItemState::Open]),
        project: Some(ProjectFilter::Project(scope.project_id)),
        initiative: arguments.initiative_id,
        waiting: arguments.waiting,
        ..ActionItemFilter::default()
    };
    let mut items = action_item::list(connection, &filter, now).await?;
    if let Some(search) = arguments.search {
        let search = search.to_lowercase();
        items.retain(|item| {
            item.title.to_lowercase().contains(&search)
                || item.notes.to_lowercase().contains(&search)
        });
    }
    let more_items = items.len() > limit;
    items.truncate(limit);

    let ids: Vec<Uuid> = items.iter().map(|item| item.id).collect();
    let memberships = action_item::memberships(connection, &ids).await?;
    let summaries = items
        .iter()
        .map(|item| item_summary(item, &memberships))
        .collect();
    Ok((summaries, more_items))
}

/// One item of the project with everything recorded about it.
async fn item_detail(
    connection: &mut AsyncPgConnection,
    scope: WorkScope,
    item_id: Uuid,
    now: DateTime<Utc>,
) -> Result<Value, Failure> {
    let (item, memberships) = find_item(connection, scope, item_id).await?;
    let comments = action_item_comment::list(connection, item_id).await?;
    let mut history = action_item_event::list_for_item(connection, item_id).await?;
    let history_count = history.len();
    let older = history_count.saturating_sub(HISTORY_MAX);
    history.drain(..older);

    let item_projects = project::find_many(connection, &memberships.project_ids(item_id)).await?;
    let initiatives =
        initiative::find_live(connection, &memberships.initiative_ids(item_id)).await?;
    let initiatives = initiative_summaries(connection, &initiatives, now).await?;

    let mut detail = item_summary(&item, &memberships);
    detail["notes"] = json!(item.notes);
    detail["resolvedAt"] = json!(item.resolved_at);
    detail["dismissedAt"] = json!(item.dismissed_at);
    detail["projects"] = item_projects
        .iter()
        .map(|project| json!({ "id": project.id, "name": project.name }))
        .collect();
    detail["initiatives"] = json!(initiatives);
    detail["comments"] = comments.iter().map(comment_view).collect();
    detail["history"] = history.iter().map(history_view).collect();
    detail["historyCount"] = json!(history_count);
    Ok(json!({ "item": detail }))
}

/// The project's initiatives in `arguments`' states, by name, with their progress.
async fn list_initiatives(
    connection: &mut AsyncPgConnection,
    scope: WorkScope,
    arguments: InitiativesArguments,
    now: DateTime<Utc>,
) -> Result<Value, Failure> {
    let initiatives = initiatives_in_project(connection, scope, arguments, now).await?;
    Ok(json!({ "initiatives": initiatives }))
}

/// The summaries of the project's initiatives in `arguments`' states, by name.
async fn initiatives_in_project(
    connection: &mut AsyncPgConnection,
    scope: WorkScope,
    arguments: InitiativesArguments,
    now: DateTime<Utc>,
) -> Result<Vec<Value>, Failure> {
    let filter = InitiativeFilter {
        states: arguments
            .states
            .unwrap_or_else(|| vec![InitiativeState::Active]),
        project: Some(ProjectFilter::Project(scope.project_id)),
        deleted: false,
    };
    let initiatives = initiative::list(connection, &filter).await?;
    initiative_summaries(connection, &initiatives, now).await
}

/// One initiative of the project with its description and the project's items in it, in
/// every state, newest first and at most [`ITEMS_MAX`] of them.
///
/// Members outside the project are counted, not shown: the session reaches its own
/// project's items only, while progress counts every member.
async fn initiative_detail(
    connection: &mut AsyncPgConnection,
    scope: WorkScope,
    initiative_id: Uuid,
    now: DateTime<Utc>,
) -> Result<Value, Failure> {
    let found = find_initiative(connection, scope, initiative_id).await?;
    let mut summary = initiative_summaries(connection, std::slice::from_ref(&found), now)
        .await?
        .pop()
        .expect("one initiative in, one summary out");
    summary["description"] = json!(found.description);

    let members = ItemsArguments {
        states: Some(vec![
            ActionItemState::Inbox,
            ActionItemState::Open,
            ActionItemState::Resolved,
            ActionItemState::Dismissed,
        ]),
        initiative_id: Some(initiative_id),
        limit: Some(ITEMS_MAX),
        ..ItemsArguments::default()
    };
    let (items, more_items) = items_in_project(connection, scope, members, now).await?;
    let outside_project = action_item::count_in_initiative_outside_project(
        connection,
        initiative_id,
        scope.project_id,
    )
    .await?;
    summary["items"] = json!(items);
    summary["moreItems"] = json!(more_items);
    summary["itemsOutsideProject"] = json!(outside_project);
    Ok(json!({ "initiative": summary }))
}

/// A live initiative of the session's project.
///
/// # Errors
/// Refuses with [`ToolError::InitiativeUnavailable`] an initiative that does not exist, is
/// deleted, or is not in the project.
async fn find_initiative(
    connection: &mut AsyncPgConnection,
    scope: WorkScope,
    initiative_id: Uuid,
) -> Result<Initiative, Failure> {
    let unavailable = || Failure::Refused(ToolError::InitiativeUnavailable(initiative_id));
    let found = initiative::find(connection, initiative_id)
        .await
        .map_err(|error| match error {
            DieselError::NotFound => unavailable(),
            other => Failure::Database(other),
        })?;
    let project_ids = initiative::project_ids(connection, &[initiative_id]).await?;
    let is_in_project = project_ids
        .get(&initiative_id)
        .is_some_and(|ids| ids.contains(&scope.project_id));
    if found.deleted_at.is_some() || !is_in_project {
        return Err(unavailable());
    }
    Ok(found)
}

/// Writes `arguments.body` on an item of the project as the session.
async fn comment_on_item(
    connection: &mut AsyncPgConnection,
    scope: WorkScope,
    arguments: CommentArguments,
    now: DateTime<Utc>,
) -> Result<Recorded<Comment>, Failure> {
    let body = arguments.body.trim();
    if body.is_empty() || body.chars().count() > COMMENT_MAX_CHARACTERS {
        return Err(ToolError::Invalid(format!(
            "body must hold text, at most {COMMENT_MAX_CHARACTERS} characters"
        ))
        .into());
    }
    find_item(connection, scope, arguments.item_id).await?;
    let written = action_item_comment::create(
        connection,
        arguments.item_id,
        body.to_owned(),
        Actor::Session(scope.session_id),
        now,
    )
    .await?;
    Ok(written)
}

/// A live item of the session's project, with its memberships.
///
/// # Errors
/// Refuses with [`ToolError::ItemUnavailable`] an item that does not exist, is deleted, or
/// is not in the project.
async fn find_item(
    connection: &mut AsyncPgConnection,
    scope: WorkScope,
    item_id: Uuid,
) -> Result<(ActionItem, Memberships), Failure> {
    let unavailable = || Failure::Refused(ToolError::ItemUnavailable(item_id));
    let item = action_item::find(connection, item_id)
        .await
        .map_err(|error| match error {
            DieselError::NotFound => unavailable(),
            other => Failure::Database(other),
        })?;
    let memberships = action_item::memberships(connection, &[item_id]).await?;
    let is_in_project = memberships.project_ids(item_id).contains(&scope.project_id);
    if item.deleted_at.is_some() || !is_in_project {
        return Err(unavailable());
    }
    Ok((item, memberships))
}

/// `initiatives` with their progress now, as the agent reads them.
async fn initiative_summaries(
    connection: &mut AsyncPgConnection,
    initiatives: &[Initiative],
    now: DateTime<Utc>,
) -> Result<Vec<Value>, Failure> {
    let ids: Vec<Uuid> = initiatives.iter().map(|initiative| initiative.id).collect();
    let spans = initiative::memberships(connection, &ids).await?;
    Ok(initiatives
        .iter()
        .map(|initiative| {
            let progress: Progress = spans
                .get(&initiative.id)
                .map_or_else(Progress::default, |spans| progress::at(spans, now));
            json!({
                "id": initiative.id,
                "name": initiative.name,
                "state": initiative.state,
                "targetAt": initiative.target_at,
                "progress": progress,
            })
        })
        .collect())
}

/// An item as lists show it: everything but its notes and records.
fn item_summary(item: &ActionItem, memberships: &Memberships) -> Value {
    json!({
        "id": item.id,
        "title": item.title,
        "state": item.state,
        "priority": item.priority,
        "dueAt": item.due_at,
        "snoozedUntil": item.snoozed_until,
        "waitingOn": item.waiting_on,
        "owner": item.owner(),
        "initiativeIds": memberships.initiative_ids(item.id),
        "createdAt": item.created_at,
        "updatedAt": item.updated_at,
    })
}

fn comment_view(comment: &Comment) -> Value {
    json!({
        "id": comment.id,
        "author": comment.author,
        "body": comment.body,
        "createdAt": comment.created_at,
    })
}

fn history_view(entry: &HistoryEntry) -> Value {
    json!({
        "kind": entry.kind,
        "actor": entry.actor,
        "data": entry.data,
        "at": entry.created_at,
    })
}

#[cfg(test)]
mod tests;
