// Copyright © 2026 Jalapeno Labs

//! The company doc as the MCP server, agents' tools, meetings, and the review share it. See
//! `docs/knowledge.md`, The company doc.
//!
//! The doc's sections and their rules are `crate::models::company_doc`, and a suggestion to one is a
//! suggestion to its page (`crate::models::page_suggestion`). This module is what every caller sees
//! of them: the doc as it is answered, with each section's pending suggestions beside it as its
//! `proposals` ([`read`]), and suggesting an edit to a section by its key ([`propose`]), which
//! agents' `company_doc_propose` names. Both keep the shapes agents read before suggestions were
//! every page's, so connected agents and their guidance keep working. The review itself, a
//! workflow on the engine, is [`workflow`].

pub mod review;
pub mod workflow;

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::QueryResult;
use diesel_async::AsyncPgConnection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::agent::Author;
use crate::models::company_doc::{self, DocSection, Section, SectionSuggestion};
use crate::models::page_suggestion::{self, Edit, NewSuggestion};
use crate::realtime::EventBus;
use crate::routes::v1::pages;

/// The doc as clients and agents see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanyDocResponse {
    /// Every section, in the order the doc is read.
    pub sections: Vec<SectionResponse>,
}

/// One section, with the edits suggested to it that wait for a person.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionResponse {
    pub section: Section,
    /// What belongs there, for the agents that read and suggest to it.
    pub purpose: &'static str,
    /// Markdown.
    pub content: String,
    pub updated_at: DateTime<Utc>,
    /// Suggestions waiting on this section, oldest first.
    pub proposals: Vec<ProposalResponse>,
}

/// The agent that acted for whoever suggested an edit.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResponse {
    /// Its recorded form: `mcp:<client id>`, `session:<number>`, or `project_manager:<id>`.
    pub id: String,
    /// What an MCP client called itself, if anything.
    pub name: Option<String>,
}

/// A suggestion to a section, as agents have always read the doc's proposals.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalResponse {
    pub id: Uuid,
    pub section: String,
    /// The section as it read when the edit was suggested, and as the edit would leave it.
    pub base_content: String,
    pub content: String,
    pub reason: String,
    pub quote: Option<String>,
    pub source: Option<String>,
    /// The timeline record the review read it in, while that record exists.
    pub timeline_record_id: Option<Uuid>,
    /// The review's run that suggested it, while that run exists.
    pub workflow_run_id: Option<Uuid>,
    /// `pending`, `accepted`, or `dismissed`.
    pub state: String,
    pub decided_by: Option<Uuid>,
    pub decided_at: Option<DateTime<Utc>>,
    /// Who suggested it: a person, or the machine user of Elysia or a Project Manager.
    pub created_by: Uuid,
    pub agent: Option<AgentResponse>,
    pub created_at: DateTime<Utc>,
}

impl From<SectionSuggestion> for ProposalResponse {
    fn from(suggested: SectionSuggestion) -> Self {
        let suggestion = suggested.suggestion;
        Self {
            id: suggestion.id,
            section: suggested.section.as_str().to_owned(),
            base_content: suggestion.base_body,
            content: suggestion.body,
            reason: suggestion.reason,
            quote: suggestion.quote,
            source: suggestion.source,
            timeline_record_id: suggestion.timeline_record_id,
            workflow_run_id: suggestion.workflow_run_id,
            state: suggestion.state,
            decided_by: suggestion.decided_by,
            decided_at: suggestion.decided_at,
            created_by: suggestion.created_by,
            agent: suggestion.agent.map(|id| AgentResponse {
                id,
                name: suggestion.agent_name,
            }),
            created_at: suggestion.created_at,
        }
    }
}

/// The doc as it stands, each section with its pending suggestions.
///
/// # Errors
/// Propagates any database error.
pub async fn read(connection: &mut AsyncPgConnection) -> QueryResult<CompanyDocResponse> {
    let sections = company_doc::sections(connection).await?;
    let pending = company_doc::pending(connection).await?;
    Ok(respond(sections, pending))
}

/// `sections` as answered, each with the suggestions of `pending` made to it.
pub fn respond(sections: Vec<DocSection>, pending: Vec<SectionSuggestion>) -> CompanyDocResponse {
    let mut by_section: HashMap<Section, Vec<ProposalResponse>> = HashMap::new();
    for suggested in pending {
        by_section
            .entry(suggested.section)
            .or_default()
            .push(ProposalResponse::from(suggested));
    }
    let sections = sections
        .into_iter()
        .map(|section| SectionResponse {
            purpose: section.section.purpose(),
            proposals: by_section.remove(&section.section).unwrap_or_default(),
            section: section.section,
            content: section.content,
            updated_at: section.updated_at,
        })
        .collect();
    CompanyDocResponse { sections }
}

/// An edit to suggest to one section of the company doc, for a person to accept or dismiss: what
/// `page_suggest` takes, with the section named by its key and suggested against as it reads now.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProposeRequest {
    /// The section to change, by its key.
    pub section: Section,
    /// The changes, applied in order to the section as it reads now, as Claude Code's Edit
    /// tool takes them: `find` is text the section holds exactly once, copied exactly, and
    /// `replace` takes its place. An empty `find` adds `replace` at the end of the section. At
    /// most 20.
    pub edits: Vec<Edit>,
    /// Why the doc should say this, in a sentence or two the person reviewing it reads first.
    pub reason: String,
    /// The words that show it, quoted exactly, such as how a person signs their own name.
    #[serde(default)]
    pub quote: Option<String>,
    /// Where the quote is from, such as a meeting's title or a channel and its date.
    #[serde(default)]
    pub source: Option<String>,
}

/// A suggestion, and whether suggesting it stored it or found the same change already pending.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposedResponse {
    pub proposal: ProposalResponse,
    /// The page of the section it was suggested to, whose Suggestions tab reviews it.
    pub page_id: Uuid,
    /// `false` when the same change was already waiting for review.
    pub created: bool,
    /// What happens next, for the agent that suggested it.
    pub message: &'static str,
}

/// Suggests `request` to its section's page as `author`, announcing it when it is new.
///
/// # Errors
/// What [`NewSuggestion::new`] and [`page_suggestion::suggest`] refuse, as the routes answer them,
/// and a database error.
pub async fn propose(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    request: ProposeRequest,
    author: Author<'_>,
) -> Result<ProposedResponse, ApiError> {
    let section = request.section;
    let new = NewSuggestion::new(
        section.page_id(),
        None,
        request.edits,
        &request.reason,
        request.quote.as_deref(),
        request.source.as_deref(),
    )?;
    let suggested = page_suggestion::suggest(connection, new, author).await?;
    let created = suggested.created;
    let proposal = ProposalResponse::from(SectionSuggestion {
        section,
        suggestion: suggested.suggestion.clone(),
    });
    pages::announce_suggested(events, connection, suggested).await;
    Ok(ProposedResponse {
        proposal,
        page_id: section.page_id(),
        created,
        message: "Nothing in the company doc changes until a person accepts this suggestion on \
            the section's page.",
    })
}
