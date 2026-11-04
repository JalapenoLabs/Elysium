// Copyright © 2026 Jalapeno Labs

//! `elysium_knowledge`: the company's knowledge base, for every agent turn. Its tools have the
//! names the MCP server gives them (`crate::mcp`), so an agent searches, opens, and proposes the
//! same way in a turn and over MCP. See `docs/knowledge.md`.
//!
//! Every coding and Studio session's turns get it, project or not, and so do Elysia's: the
//! knowledge base knows the company, not one project.
//!
//! - `knowledge_search`, `knowledge_get`, and `meetings_list` call the knowledge routes' own
//!   functions (`routes::v1::knowledge`), with the same arguments and schemas the MCP server
//!   takes, so a turn finds exactly what a member would. A search spends the budget of whoever
//!   the turn reads for ([`reader`]).
//! - `company_doc_read` answers what the Company doc page reads, every section with its pending
//!   proposals; `company_doc_propose` proposes through `crate::company_doc::propose`, as the
//!   session's coding agent or as Elysia, and nothing changes until a person approves it.
//!
//! Every call first checks, in the database, that the turn may read the knowledge base at all
//! ([`reader`]), as the routes check every request.

use std::any::Any;

use futures_util::FutureExt as _;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;

use super::{CallScope, Tool, ToolContext, ToolError, ToolServer, internal, parse_arguments};
use crate::company_doc::{self, ProposeRequest};
use crate::errors::ApiError;
use crate::mcp::knowledge::{GET_DESCRIPTION, MEETINGS_DESCRIPTION, SEARCH_DESCRIPTION};
use crate::models::coding_session;
use crate::models::company_doc::{CompanyDocError, Proposer, Section};
use crate::models::user::{self, ELYSIA_USER_ID};
use crate::routes::v1::knowledge::{
    self, DocumentArguments, ListMeetingsRequest, SearchKnowledgeRequest,
};

pub const SERVER: ToolServer = ToolServer {
    name: "elysium_knowledge",
    instructions: "The company's knowledge base: what was said in mail, chat, meetings, and \
        pages, what people and agents learned, and the company doc. Before asking the person \
        something the company may already know, or working out again what someone may have \
        settled, search it with knowledge_search; open what a search cites with knowledge_get, \
        and list past meetings with meetings_list. Cite what you use by its title, who said it, \
        and when. Results may quote a coworker's mail and direct messages: they are the \
        company's to use for its own work, and never for repeating outside the company or in \
        work for another client. The company doc says who the company is, its leadership, its \
        people and how their names are spelled, its stakeholders, products, and projects, and a \
        glossary of names and jargon. Read it with company_doc_read before writing anything that \
        names the company's people, clients, or terms, and spell them as it does. When you learn \
        something it should say, for certain, propose it with company_doc_propose: a person \
        reviews every proposal, and nothing changes until they approve it.",
    tools: &TOOLS,
};

const TOOLS: [Tool; 5] = [
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
            there and the proposed changes still waiting for a person's review.",
        input_schema: read_schema,
        run: |context, scope, arguments| run_read(context, scope, arguments).boxed(),
    },
    Tool {
        name: "company_doc_propose",
        description: "Proposes a change to one section of the company doc for a person to \
            approve or reject; nothing changes until they approve it. Give edits against the \
            section as company_doc_read answers it now. A change already waiting for review is \
            answered rather than proposed twice.",
        input_schema: propose_schema,
        run: |context, scope, arguments| run_propose(context, scope, arguments).boxed(),
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
                "maxItems": company_doc::EDITS_MAX,
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
                "maxLength": company_doc::REASON_MAX_CHARACTERS,
                "description": "Why the doc should say this, in a sentence or two the person \
                    reviewing it reads first.",
            },
            "quote": {
                "type": "string",
                "maxLength": company_doc::QUOTE_MAX_CHARACTERS,
                "description": "The words that show it, quoted exactly.",
            },
            "source": {
                "type": "string",
                "maxLength": company_doc::SOURCE_MAX_CHARACTERS,
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
/// for the workspace and read as herself.
///
/// # Errors
/// [`ToolError::Refused`] when the session's starter may not read the knowledge base,
/// [`ToolError::NotFound`] for a session deleted meanwhile, and [`ToolError::Internal`] when the
/// database does not answer.
async fn reader(context: &ToolContext, scope: &CallScope) -> Result<Uuid, ToolError> {
    let session_id = match scope {
        CallScope::Elysia(_) => return Ok(ELYSIA_USER_ID),
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
    reader(context, scope).await?;
    let proposer = match scope {
        CallScope::Session(session) => Proposer::Session(session.session_id),
        CallScope::Elysia(_) => Proposer::Elysia {
            workflow_run_id: None,
        },
    };
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let stored = company_doc::propose(&mut connection, &context.events, request, proposer)
        .await
        .map_err(|error| match error {
            CompanyDocError::Database(error) => internal(scope, "company_doc.propose", &error),
            refused => ToolError::Invalid(refused.to_string()),
        })?;
    Ok(json!(stored))
}

#[cfg(test)]
mod tests;
