// Copyright © 2026 Jalapeno Labs

//! `elysium_knowledge`: the company's knowledge base, for every agent turn. Its tools have the
//! names the MCP server gives them (`crate::mcp`), so an agent searches, opens, and proposes the
//! same way in a turn and over MCP. See `docs/knowledge.md`.
//!
//! Every coding and Studio session's turns get it, project or not, and so do Elysia's and every
//! Project Manager's: the knowledge base knows the company, not one project.
//!
//! - `knowledge_search`, `knowledge_get`, and `meetings_list` call the knowledge routes' own
//!   functions (`routes::v1::knowledge`), with the same arguments and schemas the MCP server
//!   takes, so a turn finds exactly what a member would. A search spends the budget of whoever
//!   the turn reads for ([`reader`]).
//! - `company_doc_read` answers every section with the suggestions waiting on it, as its
//!   proposals; `company_doc_propose` is `page_suggest` for one section, through
//!   `crate::company_doc::propose`, and nothing changes until a person accepts it.
//! - `page_read`, `page_list`, `page_save`, `page_suggest`, `page_move`, `page_label_add`,
//!   `page_label_remove`, `page_project_link`, and `page_project_unlink` call the page routes' own
//!   functions (`routes::v1::pages`) with the arguments the MCP server takes. A change or a
//!   suggestion is made by whoever the turn acts for and names its agent ([`writer`]), so it is
//!   recorded like any other. A pool worker reads pages and changes or suggests nothing.
//!
//! Every call first checks, in the database, that the turn may read the knowledge base at all
//! ([`reader`]), as the routes check every request.

use std::any::Any;

use chrono::Utc;
use futures_util::FutureExt as _;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;

use super::{
    CallScope, Instructions, Tool, ToolContext, ToolError, ToolServer, internal, parse_arguments,
};
use crate::agent_guidance::Audience;
use crate::company_doc::{self, ProposeRequest};
use crate::errors::ApiError;
use crate::mcp::knowledge::{GET_DESCRIPTION, MEETINGS_DESCRIPTION, SEARCH_DESCRIPTION};
use crate::mcp::pages::{
    LABEL_ADD_DESCRIPTION, LABEL_REMOVE_DESCRIPTION, LIST_DESCRIPTION, MOVE_DESCRIPTION,
    PROJECT_LINK_DESCRIPTION, PROJECT_UNLINK_DESCRIPTION, READ_DESCRIPTION, SAVE_DESCRIPTION,
    SUGGEST_DESCRIPTION,
};
use crate::models::agent::{Agent, Author};
use crate::models::coding_session;
use crate::models::company_doc::Section;
use crate::models::page_suggestion;
use crate::models::user::{self, CODING_AGENT_USER_ID, ELYSIA_USER_ID, PROJECT_MANAGER_USER_ID};
use crate::routes::v1::knowledge::{
    self, DocumentArguments, ListMeetingsRequest, SearchKnowledgeRequest,
};
use crate::routes::v1::pages::{
    self, ListPagesArguments, MovePageArguments, PAGE_IS_A_LEARNING, PageArguments,
    PageLabelArguments, PageProjectArguments, SavePageRequest, SuggestPageRequest,
};

/// Its instructions are the company's agent guidance, so an admin's edit reaches every turn as it
/// reaches every MCP client, under the rules no edit removes (`crate::agent_guidance`).
pub const SERVER: ToolServer = ToolServer {
    name: "elysium_knowledge",
    instructions: Instructions::Guidance(Audience::Turns),
    tools: &TOOLS,
};

const TOOLS: [Tool; 14] = [
    Tool {
        name: "knowledge_search",
        description: SEARCH_DESCRIPTION,
        input_schema: schema_of::<SearchKnowledgeRequest>,
        run: |context, scope, arguments| run_search(context, scope, arguments).boxed(),
    },
    Tool {
        name: "knowledge_get",
        description: GET_DESCRIPTION,
        input_schema: schema_of::<DocumentArguments>,
        run: |context, scope, arguments| run_get(context, scope, arguments).boxed(),
    },
    Tool {
        name: "meetings_list",
        description: MEETINGS_DESCRIPTION,
        input_schema: schema_of::<ListMeetingsRequest>,
        run: |context, scope, arguments| run_meetings(context, scope, arguments).boxed(),
    },
    Tool {
        name: "company_doc_read",
        description: "Reads the company doc, every section in order, each with what belongs \
            there and the suggested edits still waiting for a person's review, as its proposals.",
        input_schema: read_schema,
        run: |context, scope, arguments| run_read(context, scope, arguments).boxed(),
    },
    Tool {
        name: "company_doc_propose",
        description: "Suggests an edit to one section of the company doc for a person to \
            accept or dismiss on the section's page; nothing changes until they accept it. It is \
            page_suggest for a section named by its key. Give edits against the section as \
            company_doc_read answers it now. An edit already waiting for review is answered \
            rather than suggested twice.",
        input_schema: propose_schema,
        run: |context, scope, arguments| run_propose(context, scope, arguments).boxed(),
    },
    Tool {
        name: "page_read",
        description: READ_DESCRIPTION,
        input_schema: schema_of::<PageArguments>,
        run: |context, scope, arguments| run_page_read(context, scope, arguments).boxed(),
    },
    Tool {
        name: "page_list",
        description: LIST_DESCRIPTION,
        input_schema: schema_of::<ListPagesArguments>,
        run: |context, scope, arguments| run_page_list(context, scope, arguments).boxed(),
    },
    Tool {
        name: "page_save",
        description: SAVE_DESCRIPTION,
        input_schema: schema_of::<SavePageRequest>,
        run: |context, scope, arguments| run_page_save(context, scope, arguments).boxed(),
    },
    Tool {
        name: "page_suggest",
        description: SUGGEST_DESCRIPTION,
        input_schema: schema_of::<SuggestPageRequest>,
        run: |context, scope, arguments| run_page_suggest(context, scope, arguments).boxed(),
    },
    Tool {
        name: "page_move",
        description: MOVE_DESCRIPTION,
        input_schema: schema_of::<MovePageArguments>,
        run: |context, scope, arguments| run_page_move(context, scope, arguments).boxed(),
    },
    Tool {
        name: "page_label_add",
        description: LABEL_ADD_DESCRIPTION,
        input_schema: schema_of::<PageLabelArguments>,
        run: |context, scope, arguments| run_page_label_add(context, scope, arguments).boxed(),
    },
    Tool {
        name: "page_label_remove",
        description: LABEL_REMOVE_DESCRIPTION,
        input_schema: schema_of::<PageLabelArguments>,
        run: |context, scope, arguments| run_page_label_remove(context, scope, arguments).boxed(),
    },
    Tool {
        name: "page_project_link",
        description: PROJECT_LINK_DESCRIPTION,
        input_schema: schema_of::<PageProjectArguments>,
        run: |context, scope, arguments| run_page_project_link(context, scope, arguments).boxed(),
    },
    Tool {
        name: "page_project_unlink",
        description: PROJECT_UNLINK_DESCRIPTION,
        input_schema: schema_of::<PageProjectArguments>,
        run: |context, scope, arguments| run_page_project_unlink(context, scope, arguments).boxed(),
    },
];

/// The schema the MCP server gives the same arguments, so the two never drift: rmcp's own
/// generator, without the top-level title and description, which name the Rust type and are
/// noise to the model, as rmcp strips them for its tools.
fn schema_of<Arguments: JsonSchema + Any>() -> Value {
    let mut schema = (*schema_for_type::<Arguments>()).clone();
    schema.remove("title");
    schema.remove("description");
    Value::Object(schema)
}

fn read_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false,
    })
}

fn propose_schema() -> Value {
    let sections: Vec<&str> = Section::ALL
        .iter()
        .map(|section| section.as_str())
        .collect();
    json!({
        "type": "object",
        "properties": {
            "section": {
                "type": "string",
                "enum": sections,
                "description": "The section to change, by its key.",
            },
            "edits": {
                "type": "array",
                "minItems": 1,
                "maxItems": page_suggestion::EDITS_MAX,
                "description": "The changes, applied in order, as Claude Code's Edit tool takes \
                    them: find is text the section holds exactly once, copied exactly, and \
                    replace takes its place. An empty find adds replace at the end of the \
                    section.",
                "items": {
                    "type": "object",
                    "properties": {
                        "find": { "type": "string" },
                        "replace": { "type": "string" },
                    },
                    "required": ["find", "replace"],
                    "additionalProperties": false,
                },
            },
            "reason": {
                "type": "string",
                "minLength": 1,
                "maxLength": page_suggestion::REASON_MAX_CHARACTERS,
                "description": "Why the doc should say this, in a sentence or two the person \
                    reviewing it reads first.",
            },
            "quote": {
                "type": "string",
                "maxLength": page_suggestion::QUOTE_MAX_CHARACTERS,
                "description": "The words that show it, quoted exactly.",
            },
            "source": {
                "type": "string",
                "maxLength": page_suggestion::SOURCE_MAX_CHARACTERS,
                "description": "Where the quote is from, such as a meeting's title or a \
                    channel and its date.",
            },
        },
        "required": ["section", "edits", "reason"],
        "additionalProperties": false,
    })
}

/// Who a call reads the knowledge base for, once they are found to be allowed: whose search
/// budget it spends, re-read from the database on every call.
///
/// A session's agent acts for whoever started the session: a company member, or one of
/// Elysium's own machines. A guest, or a person disabled since, reaches none of the knowledge
/// base, as the routes refuse them (`docs/knowledge.md`, Who sees what). Elysia's own turns act
/// for the workspace and read as herself, and a Project Manager's as its machine user.
///
/// # Errors
/// [`ToolError::Refused`] when the session's starter may not read the knowledge base,
/// [`ToolError::NotFound`] for a session deleted meanwhile, and [`ToolError::Internal`] when the
/// database does not answer.
async fn reader(context: &ToolContext, scope: &CallScope) -> Result<Uuid, ToolError> {
    let session_id = match scope {
        CallScope::Elysia(_) | CallScope::Segmenting(_) => return Ok(ELYSIA_USER_ID),
        CallScope::ProjectManager(_) => return Ok(PROJECT_MANAGER_USER_ID),
        // A pool worker is the coding agent at work on its batch, which a Project Manager, not
        // a person, started.
        CallScope::Worker(_) => return Ok(CODING_AGENT_USER_ID),
        CallScope::Session(session) => session.session_id,
    };
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let session = coding_session::find(&mut connection, session_id)
        .await
        .map_err(|error| match error {
            diesel::result::Error::NotFound => {
                ToolError::NotFound(format!("session {session_id} no longer exists"))
            }
            other => internal(scope, "knowledge.session", &other),
        })?;
    let starter = user::find(&mut connection, session.created_by)
        .await
        .map_err(|error| internal(scope, "knowledge.starter", &error))?;
    if starter.is_company_member() || starter.is_machine() {
        return Ok(starter.id);
    }
    event!(
        name: "tools.knowledge.refused",
        Level::INFO,
        tool.caller = scope.label(),
        user.id = %starter.id,
        "refused the knowledge base to a session started by someone outside the company",
    );
    Err(ToolError::Refused(
        "the knowledge base is for the company's members alone, and whoever started this \
         session is not one; work from what the session and its repositories hold"
            .to_owned(),
    ))
}

/// Who a change to a page is by, and the agent acting for them, checked as [`reader`] checks.
///
/// A session's agent writes as whoever started the session, naming the session; a Project Manager
/// as its machine user, naming its project; and Elysia as herself, with no agent to name. A pool
/// worker is refused: it runs unattended for a batch rather than a person, nothing identifies it in
/// a version, and writing up what it learned is its Project Manager's, which reads its result.
///
/// # Errors
/// [`ToolError::WrongAgent`] for a worker, and what [`reader`] refuses.
async fn writer(
    context: &ToolContext,
    scope: &CallScope,
) -> Result<(Uuid, Option<Agent>), ToolError> {
    let agent = match scope {
        CallScope::Session(session) => Some(Agent::session(session.session_id)),
        CallScope::ProjectManager(manager) => Some(Agent::project_manager(manager.project_id)),
        CallScope::Elysia(_) => None,
        CallScope::Worker(_) | CallScope::Segmenting(_) => {
            return Err(ToolError::WrongAgent(
                "coding sessions, Project Managers, and Elysia; tell your Project Manager \
                 what the page should say in your worker_finish result",
            ));
        }
    };
    Ok((reader(context, scope).await?, agent))
}

/// A page route's refusal as the agent reads it, with the code its tool description names. A
/// learning's points at nothing to call, since a turn has no `learning_save`.
fn page_tool_error(scope: &CallScope, operation: &'static str, error: ApiError) -> ToolError {
    match error {
        ApiError::Validation(errors) => {
            ToolError::Invalid(format!("the arguments are outside their limits: {errors}"))
        }
        ApiError::BadRequest(message) => ToolError::Invalid(message),
        ApiError::NotFound => ToolError::NotFound(
            "the knowledge base holds no page with that id; find one with page_list".to_owned(),
        ),
        ApiError::CodedConflict {
            code: PAGE_IS_A_LEARNING,
            ..
        } => ToolError::Refused(format!(
            "{PAGE_IS_A_LEARNING}: this server cannot change a learning; say what it should say \
             in your answer instead"
        )),
        ApiError::CodedConflict { code, message } => {
            ToolError::Refused(format!("{code}: {message}"))
        }
        other => internal(scope, operation, &other),
    }
}

/// A knowledge route's refusal as the agent reads it: what to change, or when to try again.
fn tool_error(scope: &CallScope, operation: &'static str, error: ApiError) -> ToolError {
    match error {
        ApiError::Validation(errors) => {
            ToolError::Invalid(format!("the arguments are outside their limits: {errors}"))
        }
        ApiError::TooManyRequests { message, .. } => ToolError::Refused(message),
        ApiError::NotFound => ToolError::NotFound(
            "the knowledge base holds no document with that id; take documentId from a citation \
             that knowledge_search or meetings_list answered"
                .to_owned(),
        ),
        other => internal(scope, operation, &other),
    }
}

async fn run_search(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let request: SearchKnowledgeRequest = parse_arguments(arguments)?;
    let reader = reader(context, scope).await?;
    let found = knowledge::search(&context.knowledge, &context.database, reader, request)
        .await
        .map_err(|error| tool_error(scope, "knowledge.search", error))?;
    Ok(json!(found))
}

async fn run_get(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: DocumentArguments = parse_arguments(arguments)?;
    reader(context, scope).await?;
    let (id, span) = arguments.into_parts();
    let document = knowledge::open(&context.database, id, &span)
        .await
        .map_err(|error| tool_error(scope, "knowledge.open", error))?;
    Ok(json!({ "document": document }))
}

async fn run_meetings(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let request: ListMeetingsRequest = parse_arguments(arguments)?;
    reader(context, scope).await?;
    let meetings = knowledge::list_meetings(&context.database, request)
        .await
        .map_err(|error| tool_error(scope, "knowledge.meetings", error))?;
    Ok(json!({ "meetings": meetings }))
}

async fn run_read(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    #[expect(
        clippy::empty_structs_with_brackets,
        reason = "serde reads {} only into a braced struct; a unit struct takes null"
    )]
    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Nothing {}

    parse_arguments::<Nothing>(arguments)?;
    reader(context, scope).await?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let doc = company_doc::read(&mut connection)
        .await
        .map_err(|error| internal(scope, "company_doc.read", &error))?;
    Ok(json!(doc))
}

async fn run_propose(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let request: ProposeRequest = parse_arguments(arguments)?;
    let (user_id, agent) = writer(context, scope).await?;
    let author = Author {
        user_id,
        agent: agent.as_ref(),
        at: Utc::now(),
    };
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let stored = company_doc::propose(&mut connection, &context.events, request, author)
        .await
        .map_err(|error| page_tool_error(scope, "company_doc.propose", error))?;
    Ok(json!(stored))
}

async fn run_page_read(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: PageArguments = parse_arguments(arguments)?;
    reader(context, scope).await?;
    let (page, suggestions) = pages::read_with_suggestions(&context.database, arguments.id)
        .await
        .map_err(|error| page_tool_error(scope, "pages.read", error))?;
    Ok(json!({ "page": page, "suggestions": suggestions }))
}

async fn run_page_suggest(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let request: SuggestPageRequest = parse_arguments(arguments)?;
    let (user_id, agent) = writer(context, scope).await?;
    let author = Author {
        user_id,
        agent: agent.as_ref(),
        at: Utc::now(),
    };
    let suggested = pages::suggest(&context.database, &context.events, author, request)
        .await
        .map_err(|error| page_tool_error(scope, "pages.suggest", error))?;
    Ok(json!(suggested))
}

async fn run_page_list(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: ListPagesArguments = parse_arguments(arguments)?;
    reader(context, scope).await?;
    let branch = pages::list_branch(&context.database, arguments.parent_id, arguments.offset)
        .await
        .map_err(|error| page_tool_error(scope, "pages.list", error))?;
    Ok(json!(branch))
}

async fn run_page_save(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let request: SavePageRequest = parse_arguments(arguments)?;
    let (user_id, agent) = writer(context, scope).await?;
    let author = Author {
        user_id,
        agent: agent.as_ref(),
        at: Utc::now(),
    };
    let saved = pages::save(&context.database, &context.events, author, request)
        .await
        .map_err(|error| page_tool_error(scope, "pages.save", error))?;
    Ok(json!(saved))
}

async fn run_page_move(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: MovePageArguments = parse_arguments(arguments)?;
    let (user_id, agent) = writer(context, scope).await?;
    let author = Author {
        user_id,
        agent: agent.as_ref(),
        at: Utc::now(),
    };
    let moved = pages::move_to(
        &context.database,
        &context.events,
        author,
        arguments.id,
        arguments.to,
    )
    .await
    .map_err(|error| page_tool_error(scope, "pages.move", error))?;
    Ok(json!({ "page": moved }))
}

async fn run_page_label_add(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: PageLabelArguments = parse_arguments(arguments)?;
    let (user_id, agent) = writer(context, scope).await?;
    let author = Author {
        user_id,
        agent: agent.as_ref(),
        at: Utc::now(),
    };
    let labeled = pages::add_label(
        &context.database,
        &context.events,
        author,
        arguments.id,
        &arguments.label,
    )
    .await
    .map_err(|error| page_tool_error(scope, "pages.label_add", error))?;
    Ok(json!({ "page": labeled }))
}

async fn run_page_label_remove(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: PageLabelArguments = parse_arguments(arguments)?;
    let (user_id, agent) = writer(context, scope).await?;
    let author = Author {
        user_id,
        agent: agent.as_ref(),
        at: Utc::now(),
    };
    let unlabeled = pages::remove_label(
        &context.database,
        &context.events,
        author,
        arguments.id,
        &arguments.label,
    )
    .await
    .map_err(|error| page_tool_error(scope, "pages.label_remove", error))?;
    Ok(json!({ "page": unlabeled }))
}

async fn run_page_project_link(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: PageProjectArguments = parse_arguments(arguments)?;
    let (user_id, agent) = writer(context, scope).await?;
    let author = Author {
        user_id,
        agent: agent.as_ref(),
        at: Utc::now(),
    };
    let linked = pages::link_project(
        &context.database,
        &context.events,
        author,
        arguments.id,
        arguments.project_id,
    )
    .await
    .map_err(|error| link_tool_error(scope, "pages.project_link", error))?;
    Ok(json!({ "page": linked }))
}

async fn run_page_project_unlink(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: PageProjectArguments = parse_arguments(arguments)?;
    let (user_id, agent) = writer(context, scope).await?;
    let author = Author {
        user_id,
        agent: agent.as_ref(),
        at: Utc::now(),
    };
    let unlinked = pages::unlink_project(
        &context.database,
        &context.events,
        author,
        arguments.id,
        arguments.project_id,
    )
    .await
    .map_err(|error| link_tool_error(scope, "pages.project_unlink", error))?;
    Ok(json!({ "page": unlinked }))
}

/// A link route's refusal as the agent reads it: a link names two things, so not finding one says
/// so of both, where every other page tool's names the page alone.
fn link_tool_error(scope: &CallScope, operation: &'static str, error: ApiError) -> ToolError {
    match error {
        ApiError::NotFound => ToolError::NotFound(
            "the knowledge base holds no page, or Elysium no project, with that id; find a page \
             with page_list"
                .to_owned(),
        ),
        other => page_tool_error(scope, operation, other),
    }
}

#[cfg(test)]
mod tests;
