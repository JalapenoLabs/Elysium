// Copyright © 2026 Jalapeno Labs

//! `elysium_knowledge`: the company's knowledge base, for every agent turn. Today it holds the
//! company doc's two tools, under the names the MCP server gives them (`crate::mcp`), so an
//! agent reads and proposes to the doc the same way in a turn and over MCP. See
//! `docs/knowledge.md`, The company doc.
//!
//! Every coding and Studio session's turns get it, project or not, and so do Elysia's: the doc
//! knows the company, not one project. `company_doc_read` answers what the Company doc page
//! reads, every section with its pending proposals; `company_doc_propose` proposes through
//! `crate::company_doc::propose`, as the session's coding agent or as Elysia, and nothing
//! changes until a person approves it.

use futures_util::FutureExt as _;
use serde_json::{Value, json};

use super::{CallScope, Tool, ToolContext, ToolError, ToolServer, internal, parse_arguments};
use crate::company_doc::{self, ProposeRequest};
use crate::models::company_doc::{CompanyDocError, Proposer, Section};

pub const SERVER: ToolServer = ToolServer {
    name: "elysium_knowledge",
    instructions: "The company's knowledge base. The company doc says who the company is, its \
        leadership, its people and how their names are spelled, its stakeholders, products, and \
        projects, and a glossary of names and jargon. Read it with company_doc_read before \
        writing anything that names the company's people, clients, or terms, and spell them as \
        it does. When you learn something it should say, for certain, propose it with \
        company_doc_propose: a person reviews every proposal, and nothing changes until they \
        approve it. Everything in it is the company's own and is not for repeating outside it.",
    tools: &TOOLS,
};

const TOOLS: [Tool; 2] = [
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
mod tests {
    use super::*;

    #[test]
    fn the_tools_share_the_mcp_servers_names() {
        let names: Vec<&str> = TOOLS.iter().map(|tool| tool.name).collect();
        assert_eq!(names, ["company_doc_read", "company_doc_propose"]);
    }

    #[test]
    fn the_propose_schema_names_every_section_and_reads_as_the_request() {
        let schema = propose_schema();
        assert_eq!(
            schema["properties"]["section"]["enum"]
                .as_array()
                .map(Vec::len),
            Some(Section::ALL.len())
        );

        let request: ProposeRequest = parse_arguments(
            r#"{"section":"glossary","edits":[{"find":"","replace":"- ACME"}],"reason":"Why"}"#,
        )
        .expect("what the schema describes parses");
        assert_eq!(request.section, Section::Glossary);
        assert!(
            parse_arguments::<ProposeRequest>(r#"{"section":"glossary","edits":[]}"#).is_err(),
            "a reason is required"
        );
    }
}
